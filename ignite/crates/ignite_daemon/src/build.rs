//! Build products: one build per target directory at a time, joined by requests for the same
//! product in the same tree state, and skipped when the tree already holds it.
//!
//! A build product is a `[product.<name>]` table in the check manifests: the command that makes
//! it, the directory it runs in, what it reads and the directories it writes. A request names a
//! tree, a product and the packages the caller needs. [`BuildManager::ensure`] answers it in one
//! of three ways:
//!
//! - The tree already holds the product for the same packages and the same input hash: it
//!   answers [`BuildResult::Current`] without starting anything.
//! - A build of the same product, packages and input hash is running in the tree: it waits for
//!   that build and returns its result.
//! - The last build of the same product, packages and input hash in the tree exited with a
//!   failing code: it returns that failure again without starting anything, since the same
//!   inputs fail the same way. A build that a signal ended, or that hung, is not remembered,
//!   so the next request tries it again.
//! - Otherwise it waits until no build is writing any of the product's output directories in
//!   that tree, then runs the build command through the [`Launcher`].
//!
//! The input hash covers the product's resolved inputs (by git object ID) and its build command
//! as it will run, so an edit to anything the product reads makes the next request build.
//!
//! A request for a subset of a Cargo workspace's packages must not change the features Cargo
//! enables on the dependencies they share with the whole workspace, or the subset build would
//! rebuild those dependencies and the next whole build would rebuild them again. Whenever the
//! tree's Cargo manifests, lock files or toolchain files differ from any state already checked,
//! the manager compares `cargo tree -e features` for the subset with the whole workspace's and
//! fails the request, naming the crates that differ.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use ignite_core::hash::{HashParts, input_hash};
use ignite_core::ids::{InputHash, TreeId};
use ignite_core::inputs::{self, CrateGraph};
use ignite_core::manifest::{InputPattern, Manifest, Product};
use ignite_core::protocol::hub::CompileUnit;
use ignite_core::protocol::{Class, Usage};
use ignite_core::traits::{
    BuildResult, Builds, CommandSpec, Error, Git, Launcher, Result, RunResult,
};

/// The environment variable that names the file the compiler wrapper appends compile units to,
/// one JSON [`CompileUnit`] per line.
pub use crate::wrap::UNITS_ENV;

/// The placeholder in a product's `build` command for the packages a request names.
const PACKAGES: &str = "{packages}";

/// How long a build command may run before it is ended as hung, in seconds (two hours). Builds
/// vary far more than checks do (a cold build of a large workspace takes tens of minutes), so
/// the limit is fixed and generous: it only has to end a build that will never finish. Time
/// spent waiting for cores does not count.
const BUILD_HANG_LIMIT_S: u64 = 2 * 60 * 60;

/// Files whose change can alter how Cargo resolves features: a change to any of them reruns
/// the feature comparison, and their state keys the cached crate graph.
const CARGO_FILES: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain",
    "rust-toolchain.toml",
];

/// What the manager needs to know about a tree beyond its git objects.
pub trait TreeFacts: Send + Sync {
    /// The tree's working directory.
    fn path(&self, tree: &TreeId) -> Option<PathBuf>;
    /// Every file git tracks in the tree, relative to it with `/` separators.
    fn tracked(&self, path: &Path) -> Result<Vec<String>>;
    /// The workspace packages of every Cargo workspace in the tree.
    fn crate_graph(&self, path: &Path, tracked: &[String]) -> Result<CrateGraph>;
}

/// Trees in one directory, each a subdirectory named by its ID, read with `git` and `cargo`.
#[derive(Clone, Debug)]
pub struct DirectoryTrees {
    /// The directory that holds the trees (`repository.trees` in `ignite.toml`).
    pub root: PathBuf,
}

impl TreeFacts for DirectoryTrees {
    fn path(&self, tree: &TreeId) -> Option<PathBuf> {
        let path = self.root.join(tree.as_str());
        path.is_dir().then_some(path)
    }

    fn tracked(&self, path: &Path) -> Result<Vec<String>> {
        ignite_git::tracked::files(path)
    }

    /// Runs `cargo metadata` for each workspace, which a tracked lock file marks.
    fn crate_graph(&self, path: &Path, tracked: &[String]) -> Result<CrateGraph> {
        let mut graph = CrateGraph::default();
        let root = path.to_string_lossy();
        for lock in tracked.iter().filter(|p| file_name(p) == "Cargo.lock") {
            let manifest = path.join(lock).with_file_name("Cargo.toml");
            let output = Command::new("cargo")
                .args([
                    "metadata",
                    "--format-version",
                    "1",
                    "--no-deps",
                    "--manifest-path",
                ])
                .arg(&manifest)
                .output()?;
            if !output.status.success() {
                return Err(Error::Io(format!(
                    "cargo metadata for {} failed: {}",
                    manifest.display(),
                    String::from_utf8_lossy(&output.stderr).trim()
                )));
            }
            let text = String::from_utf8_lossy(&output.stdout);
            graph
                .add_workspace(&text, &root)
                .map_err(|e| Error::Invalid(e.to_string()))?;
        }
        Ok(graph)
    }
}

/// How the manager runs builds.
pub struct BuildSettings {
    pub manifest: Manifest,
    /// Where build output notes and compile unit files are written.
    pub records: PathBuf,
    /// The cores each build asks the scheduler for.
    pub cores: u32,
    /// The class builds run in.
    pub class: Class,
    /// The `ignite` executable that the compiler wrapper and test runner scripts call. When it
    /// is empty (the daemon could not find its own executable), builds run without the wrapper
    /// and record no compile units.
    pub ignite: PathBuf,
    /// The directory the wrapper scripts are written to before each build.
    pub shims: PathBuf,
}

/// Runs build products in build trees. See the module documentation.
pub struct BuildManager {
    settings: BuildSettings,
    launcher: Arc<dyn Launcher>,
    git: Arc<dyn Git>,
    trees: Arc<dyn TreeFacts>,
    state: Mutex<State>,
    /// Signalled whenever a build ends, for requests waiting on an output directory.
    ended: Condvar,
    next_file: AtomicU64,
}

/// A product, its packages and the tree state it was built from.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Request {
    tree: TreeId,
    product: String,
    packages: Vec<String>,
}

#[derive(Default)]
struct State {
    /// The input hash each request was last built at.
    held: HashMap<Request, InputHash>,
    /// The input hash each request last failed at with an exit code, and that build's output.
    failed: HashMap<Request, (InputHash, PathBuf)>,
    /// Builds running now, by request and input hash.
    flights: HashMap<(Request, InputHash), Arc<Flight>>,
    /// The (tree, output directory) pairs a running build writes.
    busy: HashSet<(TreeId, String)>,
    /// Crate graphs by the state of the Cargo files they were read from.
    graphs: HashMap<InputHash, CrateGraph>,
    /// Feature comparisons already made, by directory, Cargo file state and packages: the
    /// crates that differ, empty when none do.
    features: HashMap<(String, InputHash, Vec<String>), Vec<String>>,
    /// The whole workspace's `cargo tree -e features` output, by directory and Cargo file state.
    whole_trees: HashMap<(String, InputHash), String>,
    /// Requests waiting for a running build of the same state.
    joined: usize,
    /// Requests waiting for an output directory to be free.
    queued: usize,
}

/// One running build that other requests can wait for.
#[derive(Default)]
struct Flight {
    result: Mutex<Option<BuildResult>>,
    done: Condvar,
}

impl Flight {
    fn wait(&self) -> BuildResult {
        let mut result = lock(&self.result);
        loop {
            if let Some(result) = result.as_ref() {
                return result.clone();
            }
            result = self
                .done
                .wait(result)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    fn finish(&self, result: BuildResult) {
        *lock(&self.result) = Some(result);
        self.done.notify_all();
    }
}

/// A build in progress: the output directories it holds and the flight that joiners wait on.
/// [`FlightGuard::finish`] ends it with the build's result. Dropped without that call, as when
/// the build panics, it frees the directories and ends the flight with a failure, so that no
/// later request waits on a build that no longer runs.
struct FlightGuard<'a> {
    manager: &'a BuildManager,
    plan: &'a Plan,
    dirs: Vec<(TreeId, String)>,
    flight_key: (Request, InputHash),
    flight: Arc<Flight>,
    finished: bool,
}

impl FlightGuard<'_> {
    fn release(&self) {
        let mut state = lock(&self.manager.state);
        for dir in &self.dirs {
            state.busy.remove(dir);
        }
        state.flights.remove(&self.flight_key);
    }

    /// Ends the build with `result`. `remember` says whether a failure is one the same inputs
    /// would repeat, so that later requests get it without building.
    fn finish(mut self, result: &BuildResult, remember: bool) {
        self.finished = true;
        let mut state = lock(&self.manager.state);
        for dir in &self.dirs {
            state.busy.remove(dir);
        }
        state.flights.remove(&self.flight_key);
        let request = &self.plan.request;
        match result {
            BuildResult::Built { .. } => {
                state.failed.remove(request);
                state.held.insert(request.clone(), self.plan.hash.clone());
            }
            BuildResult::Failed { output, .. } => {
                // A failed build may have rewritten part of its outputs, so the tree no longer
                // holds the last state that was built.
                state.held.remove(request);
                if remember {
                    let failure = (self.plan.hash.clone(), output.clone());
                    state.failed.insert(request.clone(), failure);
                }
            }
            BuildResult::Current => {}
        }
        drop(state);
        self.flight.finish(result.clone());
        self.manager.ended.notify_all();
    }
}

impl Drop for FlightGuard<'_> {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        self.release();
        let failed = self.manager.failure(
            &self.plan.request.product,
            "the build stopped without a result",
        );
        self.flight.finish(failed);
        self.manager.ended.notify_all();
    }
}

/// Everything a build of one request needs, worked out before any lock is taken.
struct Plan {
    request: Request,
    product: Product,
    path: PathBuf,
    hash: InputHash,
    args: Vec<String>,
    /// The state of the tree's Cargo files.
    cargo_state: InputHash,
    /// Whether the product reads crates, which makes the feature comparison apply.
    reads_crates: bool,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

impl BuildManager {
    pub fn new(
        settings: BuildSettings,
        launcher: Arc<dyn Launcher>,
        git: Arc<dyn Git>,
        trees: Arc<dyn TreeFacts>,
    ) -> Self {
        Self {
            settings,
            launcher,
            git,
            trees,
            state: Mutex::default(),
            ended: Condvar::new(),
            next_file: AtomicU64::new(0),
        }
    }

    /// Drops what the manager believes `tree` holds and the failures it remembers there, for
    /// when its build directories are deleted, so the next request builds.
    pub fn forget(&self, tree: &TreeId) {
        let mut state = lock(&self.state);
        state.held.retain(|request, _| request.tree != *tree);
        state.failed.retain(|request, _| request.tree != *tree);
    }

    /// Requests waiting now: those joined to a running build, and those queued behind another
    /// build writing the same output directory.
    pub fn waiting(&self) -> (usize, usize) {
        let state = lock(&self.state);
        (state.joined, state.queued)
    }

    fn plan(&self, tree: &TreeId, name: &str, packages: &[String]) -> Result<Plan> {
        let product = self
            .settings
            .manifest
            .products
            .get(name)
            .ok_or_else(|| Error::NotFound(format!("no product named {name:?} is declared")))?
            .clone();
        let path = self
            .trees
            .path(tree)
            .ok_or_else(|| Error::NotFound(format!("no build tree {tree}")))?;
        let mut packages = packages.to_vec();
        packages.sort();
        packages.dedup();
        let tracked = self.trees.tracked(&path)?;
        let cargo_files: Vec<String> = tracked
            .iter()
            .filter(|p| CARGO_FILES.contains(&file_name(p)))
            .cloned()
            .collect();
        let cargo_state = hash_of(&self.git.object_ids(&path, &cargo_files)?, &[]);
        let graph = self.graph(&path, &tracked, &cargo_state)?;
        let reads_crates = product
            .inputs
            .iter()
            .any(|p| !matches!(p, InputPattern::Path(_)));
        let patterns = expand(&product, &packages, &graph)?;
        let paths = inputs::resolve(&patterns, None, &graph, &tracked)
            .map_err(|e| Error::Invalid(format!("product.{name}.inputs: {e}")))?;
        let ids = inputs::object_ids(self.git.as_ref(), &path, &paths)?;
        let args = command(&product.build, &packages);
        let hash = hash_of(&ids, &[args.join(" ")]);
        Ok(Plan {
            request: Request {
                tree: tree.clone(),
                product: name.to_owned(),
                packages,
            },
            product,
            path,
            hash,
            args,
            cargo_state,
            reads_crates,
        })
    }

    fn graph(&self, path: &Path, tracked: &[String], state: &InputHash) -> Result<CrateGraph> {
        if let Some(graph) = lock(&self.state).graphs.get(state) {
            return Ok(graph.clone());
        }
        let graph = self.trees.crate_graph(path, tracked)?;
        lock(&self.state)
            .graphs
            .insert(state.clone(), graph.clone());
        Ok(graph)
    }

    fn cwd(plan: &Plan) -> PathBuf {
        plan.product
            .dir
            .as_ref()
            .map_or_else(|| plan.path.clone(), |dir| plan.path.join(dir))
    }

    fn spec(
        &self,
        plan: &Plan,
        args: Vec<String>,
        cores: u32,
        env: Vec<(String, String)>,
    ) -> CommandSpec {
        CommandSpec {
            args,
            cwd: Self::cwd(plan),
            env,
            class: self.settings.class,
            cores,
            resources: Vec::new(),
            grows: plan.product.grows,
            tree: Some(plan.request.tree.clone()),
            ticket: None,
            hang_limit_s: Some(BUILD_HANG_LIMIT_S),
        }
    }

    /// The variables that make Cargo compile through `ignite rustc-wrap`, which appends each
    /// compile unit to `units_file`. Without them a build records no units, so rebuild counts
    /// and the hub's compile records have nothing to read. The scripts are checked, and rewritten
    /// if they differ, before every build, so a deleted shim directory or a daemon that moved to
    /// a new executable still gets a wrapper that calls the running `ignite`.
    fn wrapper_env(&self, units_file: &Path) -> std::result::Result<Vec<(String, String)>, String> {
        let units = (
            UNITS_ENV.to_owned(),
            units_file.to_string_lossy().into_owned(),
        );
        if self.settings.ignite.as_os_str().is_empty() {
            return Ok(vec![units]);
        }
        crate::wrap::write_shims(&self.settings.shims, &self.settings.ignite).map_err(|error| {
            format!(
                "cannot write the compiler wrapper to {}: {error}",
                self.settings.shims.display()
            )
        })?;
        Ok(crate::wrap::cargo_env(&self.settings.shims, units_file))
    }

    /// A new file name in the records directory, unique within this process.
    fn record_file(&self, plan_product: &str, extension: &str) -> PathBuf {
        let n = self.next_file.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        self.settings
            .records
            .join(format!("build-{pid}-{n}-{plan_product}.{extension}"))
    }

    /// A failed result whose output file holds `message`.
    fn failure(&self, product: &str, message: &str) -> BuildResult {
        let output = self.record_file(product, "txt");
        let written = fs::create_dir_all(&self.settings.records)
            .and_then(|()| fs::write(&output, format!("{message}\n")));
        if let Err(error) = written {
            eprintln!(
                "ignite: cannot write {}: {error}: {message}",
                output.display()
            );
        }
        BuildResult::Failed {
            output,
            usage: Usage::default(),
        }
    }

    /// Compares the features of the subset's dependencies with the whole workspace's, once per
    /// state of the Cargo files. Returns a failure when they differ or `cargo tree` fails.
    fn check_features(&self, plan: &Plan) -> Option<BuildResult> {
        if !plan.reads_crates || plan.request.packages.is_empty() {
            return None;
        }
        let dir = plan.product.dir.clone().unwrap_or_default();
        let key = (
            dir.clone(),
            plan.cargo_state.clone(),
            plan.request.packages.clone(),
        );
        let known = lock(&self.state).features.get(&key).cloned();
        let differing = if let Some(differing) = known {
            differing
        } else {
            let whole = match self.whole_feature_tree(plan, dir) {
                Ok(tree) => tree,
                Err(result) => return Some(result),
            };
            let subset = match self.feature_tree(plan, &plan.request.packages) {
                Ok(tree) => tree,
                Err(result) => return Some(result),
            };
            let differing = feature_differences(&whole, &subset);
            lock(&self.state).features.insert(key, differing.clone());
            differing
        };
        if differing.is_empty() {
            return None;
        }
        Some(self.failure(
            &plan.request.product,
            &format!(
                "building {} for {} enables different features than the whole workspace on: {}",
                plan.request.product,
                plan.request.packages.join(", "),
                differing.join(", ")
            ),
        ))
    }

    /// The whole workspace's `cargo tree -e features`, run once per state of the Cargo files.
    fn whole_feature_tree(
        &self,
        plan: &Plan,
        dir: String,
    ) -> std::result::Result<String, BuildResult> {
        let key = (dir, plan.cargo_state.clone());
        if let Some(tree) = lock(&self.state).whole_trees.get(&key) {
            return Ok(tree.clone());
        }
        let tree = self.feature_tree(plan, &[])?;
        lock(&self.state).whole_trees.insert(key, tree.clone());
        Ok(tree)
    }

    /// The output of `cargo tree -e features` for `packages`, or the whole workspace when none.
    fn feature_tree(
        &self,
        plan: &Plan,
        packages: &[String],
    ) -> std::result::Result<String, BuildResult> {
        let mut args: Vec<String> = [
            "cargo", "tree", "-e", "features", "--prefix", "none", "--format", "{p}|{f}",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        args.extend(package_args(packages));
        let run = self.launcher.run(&self.spec(plan, args, 1, Vec::new()));
        if run.exit != Some(0) {
            return Err(BuildResult::Failed {
                output: run.output,
                usage: run.usage,
            });
        }
        fs::read_to_string(&run.output).map_err(|error| {
            self.failure(
                &plan.request.product,
                &format!("cannot read {}: {error}", run.output.display()),
            )
        })
    }

    /// Runs the build, or joins or skips it, holding each output directory for one build.
    fn build(&self, plan: &Plan) -> BuildResult {
        let flight_key = (plan.request.clone(), plan.hash.clone());
        let dirs: Vec<(TreeId, String)> = plan
            .product
            .outputs
            .iter()
            .map(|dir| (plan.request.tree.clone(), dir.clone()))
            .collect();
        let mut state = lock(&self.state);
        loop {
            if state.held.get(&plan.request) == Some(&plan.hash) {
                return BuildResult::Current;
            }
            if let Some((hash, output)) = state.failed.get(&plan.request)
                && *hash == plan.hash
            {
                // Nothing ran for this request, so it reports no usage.
                return BuildResult::Failed {
                    output: output.clone(),
                    usage: Usage::default(),
                };
            }
            if let Some(flight) = state.flights.get(&flight_key).cloned() {
                state.joined += 1;
                drop(state);
                let result = flight.wait();
                lock(&self.state).joined -= 1;
                return result;
            }
            if dirs.iter().all(|dir| !state.busy.contains(dir)) {
                break;
            }
            state.queued += 1;
            state = self
                .ended
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
            state.queued -= 1;
        }
        let flight = Arc::new(Flight::default());
        state
            .flights
            .insert(flight_key.clone(), Arc::clone(&flight));
        state.busy.extend(dirs.iter().cloned());
        drop(state);

        // The guard frees the directories and wakes the joiners even if `run` panics.
        let guard = FlightGuard {
            manager: self,
            plan,
            dirs,
            flight_key,
            flight,
            finished: false,
        };
        let (result, remember) = self.run(plan);
        guard.finish(&result, remember);
        result
    }

    /// Runs the build command. The flag says whether a failure came from the command exiting
    /// with a code, which the same inputs would repeat, rather than from a signal, a hang or
    /// the daemon's own files.
    fn run(&self, plan: &Plan) -> (BuildResult, bool) {
        let units_file = self.record_file(&plan.request.product, "units");
        let prepared =
            fs::create_dir_all(&self.settings.records).and_then(|()| fs::write(&units_file, ""));
        if let Err(error) = prepared {
            let message = format!("cannot create {}: {error}", units_file.display());
            return (self.failure(&plan.request.product, &message), false);
        }
        let env = match self.wrapper_env(&units_file) {
            Ok(env) => env,
            Err(message) => return (self.failure(&plan.request.product, &message), false),
        };
        let spec = self.spec(plan, plan.args.clone(), self.settings.cores, env);
        let RunResult {
            exit,
            output,
            usage,
            hung,
        } = self.launcher.run(&spec);
        let units = read_units(&units_file);
        let _ = fs::remove_file(&units_file);
        if exit == Some(0) {
            (BuildResult::Built { usage, units }, false)
        } else {
            (
                BuildResult::Failed { output, usage },
                exit.is_some() && !hung,
            )
        }
    }
}

impl Builds for BuildManager {
    fn ensure(&self, tree: &TreeId, product: &str, packages: &[String]) -> BuildResult {
        let plan = match self.plan(tree, product, packages) {
            Ok(plan) => plan,
            Err(error) => {
                return self.failure(product, &format!("cannot build {product}: {error}"));
            }
        };
        if let Some(failed) = self.check_features(&plan) {
            return failed;
        }
        self.build(&plan)
    }
}

/// The input hash of a build: its inputs' object IDs and its command.
fn hash_of(ids: &[(String, ignite_core::ids::Oid)], commands: &[String]) -> InputHash {
    input_hash(&HashParts {
        inputs: ids,
        digest: None,
        commands,
        toolchain: "",
        platform: None,
    })
}

/// The product's input patterns with `crate-closure` replaced by a `crate:` pattern for each
/// package named, or for every package of the product's workspace when none is.
fn expand(product: &Product, packages: &[String], graph: &CrateGraph) -> Result<Vec<InputPattern>> {
    let mut patterns = Vec::new();
    for pattern in &product.inputs {
        if *pattern != InputPattern::CrateClosure {
            patterns.push(pattern.clone());
            continue;
        }
        let names: Vec<String> = if packages.is_empty() {
            let workspace = product.dir.as_deref().unwrap_or("");
            graph
                .packages
                .iter()
                .filter(|p| p.workspace == workspace)
                .map(|p| p.name.clone())
                .collect()
        } else {
            packages.to_vec()
        };
        if names.is_empty() {
            return Err(Error::Invalid(
                "crate-closure names no package: the request names none and the product's \
                 directory holds no workspace"
                    .to_owned(),
            ));
        }
        patterns.extend(names.into_iter().map(InputPattern::Crate));
    }
    Ok(patterns)
}

/// Cargo's arguments selecting `packages`, or the whole workspace when none are named.
fn package_args(packages: &[String]) -> Vec<String> {
    if packages.is_empty() {
        return vec!["--workspace".to_owned()];
    }
    packages
        .iter()
        .flat_map(|p| ["-p".to_owned(), p.clone()])
        .collect()
}

/// The build command split at whitespace, with `{packages}` replaced by Cargo's package
/// arguments. The command runs directly, not through a shell.
fn command(build: &str, packages: &[String]) -> Vec<String> {
    build
        .split_whitespace()
        .flat_map(|word| {
            if word == PACKAGES {
                package_args(packages)
            } else {
                vec![word.to_owned()]
            }
        })
        .collect()
}

/// Each package in `cargo tree -e features --format '{p}|{f}'` output with the feature sets it
/// is built with. A package can appear with more than one set, for example once as a build
/// dependency and once as a normal one. Lines without `|` are feature nodes and are skipped.
fn feature_sets(text: &str) -> BTreeMap<&str, BTreeSet<&str>> {
    let mut sets: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for line in text.lines() {
        let Some((package, features)) = line.split_once('|') else {
            continue;
        };
        let features = features.trim_end().trim_end_matches("(*)").trim_end();
        sets.entry(package.trim()).or_default().insert(features);
    }
    sets
}

/// The names of the crates whose features in the subset's tree differ from the whole
/// workspace's, sorted.
fn feature_differences(whole: &str, subset: &str) -> Vec<String> {
    let whole = feature_sets(whole);
    let mut differing: BTreeSet<String> = BTreeSet::new();
    for (package, sets) in feature_sets(subset) {
        if whole.get(package) != Some(&sets) {
            let name = package.split_whitespace().next().unwrap_or(package);
            differing.insert(name.to_owned());
        }
    }
    differing.into_iter().collect()
}

/// The compile units in a units file, skipping lines that are not one. A missing file holds
/// none, as for a build that compiled nothing.
pub fn read_units(path: &Path) -> Vec<CompileUnit> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// How many distinct crates the units recompiled, for the line `built <product> in <s> s
/// (<n> crates recompiled)`.
pub fn recompiled(units: &[CompileUnit]) -> usize {
    units
        .iter()
        .filter(|unit| unit.rebuilt)
        .map(|unit| unit.crate_name.as_str())
        .collect::<BTreeSet<_>>()
        .len()
}

#[cfg(test)]
#[path = "build_tests.rs"]
mod tests;
