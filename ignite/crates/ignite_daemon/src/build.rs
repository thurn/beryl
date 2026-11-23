//! Build products: one build per target directory at a time, joined by requests for the same
//! product in the same tree state, and skipped when the tree already holds it.
//!
//! A build product is a `[product.<name>]` table in the check manifests: the command that makes
//! it, the directory it runs in, what it reads and the directories it writes. The manager reads
//! the product from the manifests of the tree it builds in, so a tree whose `ignite/checks`
//! changes a product builds it as that tree says, whatever the daemon's own manifests hold; those
//! are used only for a tree with no manifest directory (see `crate::manifests`). A request names a
//! tree, a product and the packages the caller needs. [`BuildManager::ensure`] answers it in one
//! of three ways:
//!
//! - Before anything else, while the tree's lease copies another tree's build into it, it
//!   waits for the copy (see `crate::trees::wait_until_seeded`).
//! - The tree already holds the product for the same packages and the same input hash: it
//!   answers [`BuildResult::Current`] without starting anything.
//! - A build of the same product, packages and input hash is running in the tree: it waits for
//!   that build and returns its result.
//! - The last build of the same product, packages and input hash in the tree exited with a
//!   failing code: it returns that failure again without starting anything, since the same
//!   inputs fail the same way. A build that a signal ended, or that hung, is not remembered,
//!   so the next request tries it again.
//! - Otherwise it waits until no build is writing any of the product's output directories in
//!   that tree, then waits for a build slot and free space on the tree volume (see `room`),
//!   then runs the build command through the [`Launcher`]. A failure that looks
//!   like damage to the tree's own build products is cleared and run once more (see `repair`).
//!
//! The input hash covers the product's resolved inputs (by git object ID) and its build command
//! as it will run, so an edit to anything the product reads makes the next request build.
//!
//! A request for a subset of a Cargo workspace's packages must not change the features Cargo
//! enables on the dependencies they share with the whole workspace, or the subset build would
//! rebuild those dependencies and the next whole build would rebuild them again. Whenever the
//! tree's Cargo manifests, lock files or toolchain files differ from any state already checked,
//! the manager compares `cargo tree -e features` for the subset with the whole workspace's.
//! The whole workspace leaves out the packages in the product's `exclude`, so products and
//! families that select the same packages share one feature configuration (see `cargo`).
//! When they differ, it builds the whole workspace instead of the subset. Failing the request
//! instead would fail every check that needs the product, because a large workspace enables
//! different features for almost every subset.
//!
//! What each tree holds and which builds failed are kept in the local database's `builds` table
//! as well as in memory (see `store`), so a restarted daemon answers a request for a product the
//! tree already holds without running Cargo again. Every build command's environment names its
//! request as `IGNITE_BUILD_KEY`, which its run's `spec.json` keeps; a restarted daemon turns each
//! unfinished run with that variable back into a running build that requests join (see
//! `rejoin`).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

mod cargo;
mod execute;
mod flight;
mod rejoin;
pub mod repair;
mod room;
mod store;
pub(crate) use cargo::command;
use cargo::{WholeKey, feature_differences, package_args};
use flight::Flight;
pub use rejoin::BUILD_KEY_ENV;
pub use room::DiskRoom;

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

/// The start of the record of a build that could not start because the machine failed (a file
/// could not be opened, for one), not because the change is wrong. The landing reads it to
/// say so instead of blaming the change.
pub const NOT_STARTED: &str = "the build did not start, and the change is not at fault: ";

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

/// Build trees read from the daemon store, with facts from `git` and `cargo`.
#[derive(Clone, Debug)]
pub struct DirectoryTrees {
    pub db: Arc<Mutex<rusqlite::Connection>>,
}

impl TreeFacts for DirectoryTrees {
    fn path(&self, tree: &TreeId) -> Option<PathBuf> {
        crate::trees::root::recorded_path(&lock(&self.db), tree).filter(|path| path.is_dir())
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
    /// The manifests whose products the manager builds in a tree that has no manifest
    /// directory of its own. The daemon reads them again when the main checkout moves (see
    /// `crate::manifests`).
    pub manifest: crate::manifests::LiveManifest,
    /// The manifest directory within each tree (`repository.checks`), whose products the
    /// manager builds in that tree.
    pub checks: PathBuf,
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
    /// The disk whose free space a build waits for (see `room`); `None` never waits.
    room: Option<Arc<dyn DiskRoom>>,
    tree_control: std::sync::OnceLock<crate::trees::TreeBuildControl>,
    state: Mutex<State>,
    /// The local database that keeps `held` and `failed` across restarts; `None` keeps them in
    /// memory only.
    store: Mutex<Option<rusqlite::Connection>>,
    /// Signalled whenever a build ends, for requests waiting on an output directory.
    ended: Condvar,
    next_file: AtomicU64,
}

/// A product, its packages and the tree state it was built from.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
    flights: BTreeMap<(Request, InputHash), Arc<Flight>>,
    /// Crate graphs by the state of the Cargo files they were read from.
    graphs: HashMap<InputHash, CrateGraph>,
    /// Feature comparisons already made, by directory, Cargo file state and packages: the
    /// crates that differ, empty when none do.
    features: HashMap<(WholeKey, Vec<String>), Vec<String>>,
    /// The whole workspace's `cargo tree -e features` output.
    whole_trees: HashMap<WholeKey, String>,
    /// Requests waiting for a running build of the same state.
    joined: usize,
    /// Requests waiting for an output directory to be free.
    queued: usize,
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
            room: None,
            tree_control: std::sync::OnceLock::new(),
            state: Mutex::default(),
            store: Mutex::new(None),
            ended: Condvar::new(),
            next_file: AtomicU64::new(0),
        }
    }

    pub(crate) fn attach_trees(&self, trees: &crate::trees::Trees) {
        assert!(self.tree_control.set(trees.build_control()).is_ok());
    }

    /// Drops what the manager believes `tree` holds and the failures it remembers there, for
    /// when its build directories are deleted, so the next request builds.
    pub fn forget(&self, tree: &TreeId) {
        let mut state = lock(&self.state);
        state.held.retain(|request, _| request.tree != *tree);
        state.failed.retain(|request, _| request.tree != *tree);
        self.forget_stored(tree);
    }

    /// Requests waiting now: those joined to a running build, and those queued behind another
    /// build writing the same output directory.
    pub fn waiting(&self) -> (usize, usize) {
        let state = lock(&self.state);
        (state.joined, state.queued)
    }

    /// The manifests whose products the manager builds in `tree`: the tree's own, or the held
    /// ones when the tree has no manifest directory. Manifests the tree has but that cannot be
    /// parsed are an error, so no build runs a product the tree does not declare.
    fn manifest(&self, tree: &TreeId) -> Result<Arc<Manifest>> {
        let held = self.settings.manifest.get();
        let Some(root) = self.trees.path(tree) else {
            return Ok(held);
        };
        Ok(crate::manifests::in_tree(&root, &self.settings.checks)
            .map_err(|e| Error::Invalid(format!("the manifests of tree {tree}: {e}")))?
            .map_or(held, Arc::new))
    }

    fn plan(&self, tree: &TreeId, name: &str, packages: &[String]) -> Result<Plan> {
        let product = self
            .manifest(tree)?
            .products
            .get(name)
            .ok_or_else(|| Error::NotFound(format!("no product named {name:?} is declared")))?
            .clone();
        let path = self
            .trees
            .path(tree)
            .ok_or_else(|| Error::NotFound(format!("no build tree {tree}")))?;
        // A command without `{packages}` builds the same thing whatever the caller needs, so
        // every request for it is one request, and there is no subset whose features to check.
        let mut packages = if product.build.split_whitespace().any(|w| w == PACKAGES) {
            packages.to_vec()
        } else {
            Vec::new()
        };
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
        let args = command(&product.build, &packages, &product.exclude);
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
            // A build runs in the class of the work that asked for it and is charged to its
            // ticket: a landing's builds are `land` work, tip verification's are `tip` work.
            class: crate::steps::class().unwrap_or(self.settings.class),
            cores,
            resources: Vec::new(),
            grows: plan.product.grows,
            tree: Some(plan.request.tree.clone()),
            ticket: crate::steps::ticket(),
            hang_limit_s: Some(BUILD_HANG_LIMIT_S),
            reserve: None,
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

    /// The shared dependencies whose features the subset's build would enable differently
    /// from the whole workspace's, compared once per state of the Cargo files. Empty when the
    /// product reads no crates or builds the whole workspace. `Err` when `cargo tree` fails.
    fn differing_features(&self, plan: &Plan) -> std::result::Result<Vec<String>, BuildResult> {
        if !plan.reads_crates || plan.request.packages.is_empty() {
            return Ok(Vec::new());
        }
        let whole_key = WholeKey {
            dir: plan.product.dir.clone().unwrap_or_default(),
            cargo_state: plan.cargo_state.clone(),
            exclude: plan.product.exclude.clone(),
        };
        let key = (whole_key.clone(), plan.request.packages.clone());
        if let Some(differing) = lock(&self.state).features.get(&key).cloned() {
            return Ok(differing);
        }
        let whole = self.whole_feature_tree(plan, whole_key)?;
        let subset = self.feature_tree(plan, &plan.request.packages)?;
        let differing = feature_differences(&whole, &subset);
        lock(&self.state).features.insert(key, differing.clone());
        Ok(differing)
    }

    /// The whole workspace's `cargo tree -e features`, less the product's excluded packages,
    /// run once per state of the Cargo files.
    fn whole_feature_tree(
        &self,
        plan: &Plan,
        key: WholeKey,
    ) -> std::result::Result<String, BuildResult> {
        if let Some(tree) = lock(&self.state).whole_trees.get(&key) {
            return Ok(tree.clone());
        }
        let tree = self.feature_tree(plan, &[])?;
        lock(&self.state).whole_trees.insert(key, tree.clone());
        Ok(tree)
    }

    /// The output of `cargo tree -e features` for `packages`, or the whole workspace less the
    /// product's excluded packages when none.
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
        args.extend(package_args(packages, &plan.product.exclude));
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

    /// Runs the build command once, and returns how it ended with the file its compile units
    /// are in. `Err` is the failure of the daemon's own files, before anything ran. A build
    /// result's flag says whether a failure can be reused for the same inputs. An interruption
    /// or a failure of the daemon's own files leaves the build eligible to run again.
    fn run_once(&self, plan: &Plan) -> std::result::Result<(RunResult, PathBuf), BuildResult> {
        let units_file = self.record_file(&plan.request.product, "units");
        let prepared =
            fs::create_dir_all(&self.settings.records).and_then(|()| fs::write(&units_file, ""));
        if let Err(error) = prepared {
            let message = format!("cannot create {}: {error}", units_file.display());
            return Err(self.failure(&plan.request.product, &message));
        }
        let mut env = self
            .wrapper_env(&units_file)
            .map_err(|message| self.failure(&plan.request.product, &message))?;
        env.push(rejoin::BuildKey::of(plan).variable());
        let trees = self
            .tree_control
            .get()
            .and_then(crate::trees::TreeBuildControl::trees)
            .filter(|_| self.tracks_compiled_outputs(plan));
        if trees.is_some() {
            env.push((
                "CARGO_LOG".to_owned(),
                "cargo::core::compiler::fingerprint=debug".to_owned(),
            ));
        }
        let spec = self.spec(plan, plan.args.clone(), self.settings.cores, env);
        let result = self.launcher.run(&spec);
        if let Some(trees) = trees {
            trees.measure_build(
                &plan.request.tree,
                (result.exit == Some(0)).then(|| result.output.clone()),
            );
        }
        Ok((result, units_file))
    }

    /// The result of a build command that ended with `run`, whose compile units are in
    /// `units_file`, which it deletes. The flag is as for [`BuildManager::run_once`].
    fn outcome(run: RunResult, units_file: &Path, ended_partway: bool) -> (BuildResult, bool) {
        let RunResult {
            exit,
            output,
            usage,
            ..
        } = run;
        let units = read_units(units_file);
        let _ = fs::remove_file(units_file);
        if exit == Some(0) {
            (BuildResult::Built { usage, units }, false)
        } else {
            (BuildResult::Failed { output, usage }, !ended_partway)
        }
    }
}

impl Builds for BuildManager {
    fn express_build(
        &self,
        tree: &Path,
        output: &Path,
        stage: ignite_core::protocol::host::ExpressBuildStage,
    ) -> Result<()> {
        self.express_space(tree, output, stage)
    }

    fn forget(&self, tree: &TreeId) {
        BuildManager::forget(self, tree);
    }

    fn ensure(&self, tree: &TreeId, product: &str, packages: &[String]) -> BuildResult {
        // A tree whose lease is copying another tree's build has its target directories replaced.
        let path = self.trees.path(tree);
        if let Err(stopped) = path.map_or(Ok(()), |p| crate::trees::wait_until_seeded(&p)) {
            return self.failure(product, &stopped);
        }
        let plan = match self.plan(tree, product, packages) {
            Ok(plan) => plan,
            Err(error) => {
                // An operating-system failure says nothing about the change.
                let marker = if matches!(error, Error::Io(_)) {
                    NOT_STARTED
                } else {
                    ""
                };
                return self.failure(product, &format!("{marker}cannot build {product}: {error}"));
            }
        };
        match self.differing_features(&plan) {
            Ok(differing) if differing.is_empty() => self.build(&plan),
            // Building the subset would enable other features on shared dependencies than the
            // whole workspace does, so Cargo would rebuild them, and the next whole build would
            // rebuild them back. The whole workspace is built instead: it holds the subset's
            // binaries, built with the workspace's own features.
            Ok(_) => match self.plan(tree, product, &[]) {
                Ok(whole) => self.build(&whole),
                Err(error) => self.failure(product, &format!("cannot build {product}: {error}")),
            },
            Err(failed) => failed,
        }
    }

    /// The tree has built the product when the manager holds it there, for any packages and
    /// inputs (a build this daemon ran, or one a previous daemon recorded in `builds`), or when
    /// every directory the product writes is in the tree and not empty: those directories survive
    /// a daemon restart and are kept between tickets, and deleting a tree's builds deletes them. Products that write the same directory (the Rust workspace's test
    /// binaries and its command-line programs share `rust/target`) count as built together; the
    /// dependencies they share are compiled already, so building the other is incremental too.
    fn selected(&self, tree: &TreeId, product: &str, packages: &[String]) -> Vec<String> {
        self.selected_packages(tree, product, packages)
    }

    fn has_built(&self, tree: &TreeId, product: &str) -> bool {
        if lock(&self.state)
            .held
            .keys()
            .any(|request| request.tree == *tree && request.product == product)
        {
            return true;
        }
        self.manifest(tree).is_ok_and(|manifest| {
            manifest
                .products
                .get(product)
                .is_some_and(|declared| !declared.outputs.is_empty())
        }) && self.outputs_present(tree, product)
    }

    fn held_in(&self, tree: &TreeId, dirs: &[String]) -> BTreeSet<String> {
        self.held_products(tree, dirs)
    }

    fn copy_held(&self, from: &TreeId, to: &TreeId, dirs: &[String]) {
        self.copy_held_products(from, to, dirs);
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
                .filter(|p| p.workspace == workspace && !product.exclude.contains(&p.name))
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

/// The compile units in a units file, skipping lines that are not one. A missing file holds
/// none, as for a build that compiled nothing.
pub fn read_units(path: &Path) -> Vec<CompileUnit> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// How many distinct crates the units compiled, for the line `built <product> in <s> s
/// (<n> crates recompiled)`. A unit counts whether or not an earlier build had left its files
/// (the units' `rebuilt` flag): a build in a tree that has never compiled a crate still
/// compiled it. Runs of a test binary compile nothing.
pub fn recompiled(units: &[CompileUnit]) -> usize {
    units
        .iter()
        .filter(|unit| unit.mode != crate::wrap::RUN_MODE)
        .map(|unit| unit.crate_name.as_str())
        .collect::<BTreeSet<_>>()
        .len()
}

/// The `n` crates the units spent the most CPU seconds compiling, most first, each with its
/// seconds summed over its modes (a crate built for `check` and `test` counts once). The
/// `built` line names them, so a build that recompiled far more than a change reaches says
/// which crates it was.
pub fn costliest(units: &[CompileUnit], n: usize) -> Vec<(String, f64)> {
    let mut by_crate: BTreeMap<&str, f64> = BTreeMap::new();
    for unit in units.iter().filter(|u| u.mode != crate::wrap::RUN_MODE) {
        *by_crate.entry(unit.crate_name.as_str()).or_default() += unit.cpu_s;
    }
    let mut crates: Vec<(String, f64)> = by_crate
        .into_iter()
        .map(|(name, cpu_s)| (name.to_owned(), cpu_s))
        .collect();
    crates.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    crates.truncate(n);
    crates
}

#[cfg(test)]
#[path = "build_tests.rs"]
pub(crate) mod tests;
