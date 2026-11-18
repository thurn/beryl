//! The check runner: lists a family's checks, computes each one's input hash, looks them up in
//! the result cache, builds what the rest need and runs them with the family's adapter (see
//! "Checks" in `docs/ignite.md`).
//!
//! - Listing: a `jsonl` family's `list` command, or cargo-nextest's list for a `nextest` family.
//!   cargo-nextest can list tests only after building them, so a `nextest` listing is kept in
//!   memory under the hash of everything the family can read, and a family whose tests are all
//!   cached builds nothing while that hash holds.
//! - Lookup: one result cache request per command, for every check it needs ([`ResultCache`]).
//! - A failed check runs a second time at once. A pass then makes it flaky, which passes and is
//!   reported to the hub's flaky table. A check that a run did not report, or whose family's
//!   `list` failed, is `incomplete` and is tried at most twice more. Incomplete and flaky
//!   outcomes are never cached.
//! - Every command leaves a record with its cost, and a family answered wholly from the cache
//!   leaves one record with the verb `cached` and no cost.
//!
//! The verbs `ignite test`, `ignite check` and `ignite why` are in [`verbs`].

pub mod adapters;
mod execute;
mod plan;
mod regression;
pub mod sources;
pub(crate) mod verbs;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use ignite_core::config::Os;
use ignite_core::hash::{self, HashParts};
use ignite_core::ids::{CheckName, Commit, InputHash, Oid, SessionId, TicketId, TreeId};
use ignite_core::inputs::{self, CrateGraph};
use ignite_core::manifest::{Adapter, Family, InputPattern, Manifest, Platforms};
use ignite_core::protocol::hub::{CachedResult, ResultKey};
use ignite_core::protocol::{Class, Outcome};
use ignite_core::traits::{Error, Result};

use crate::cache::ResultCache;
use crate::server::Context;

pub use adapters::{Listed, Reported, Target};
pub use sources::{CheckHistory, HubHistory, TreeSource};
pub use verbs::{CheckVerb, GateRun, TestVerb, WhyVerb};

/// How many times one check or one family listing is tried while it comes back incomplete.
const TRIES: u32 = 3;

/// How the runner is set up on this machine.
#[derive(Clone, Debug)]
pub struct CheckSettings {
    pub manifest: Manifest,
    pub os: Os,
    /// The families this machine runs; `None` means every family.
    pub families: Option<Vec<String>>,
    /// The toolchain version every input hash includes.
    pub toolchain: String,
    /// Where result files of `jsonl` families are written.
    pub outputs: PathBuf,
    /// The cores each check command asks the scheduler for.
    pub cores: u32,
    /// The Ignite commit that records name.
    pub ignite: String,
}

/// Who runs the checks and how.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunOptions {
    /// The verb records name, such as `test`, `check`, `land` or `tip`.
    pub verb: String,
    pub class: Class,
    /// Run again even where the result cache has an answer.
    pub rerun: bool,
    pub ticket: Option<TicketId>,
    pub session: Option<SessionId>,
}

/// A check that did not pass, with what its run said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub check: CheckName,
    /// `Fail` or `Incomplete`.
    pub outcome: Outcome,
    pub at: Option<String>,
    pub message: Option<String>,
    /// The file that holds the whole output.
    pub output: Option<String>,
}

/// A build product that had to be built.
#[derive(Clone, Debug, PartialEq)]
pub struct Built {
    pub product: String,
    pub wall_s: f64,
    /// Compiler processes that rebuilt a crate.
    pub rebuilt: usize,
}

/// The result of running a set of checks.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CheckRun {
    pub outcomes: Vec<(CheckName, Outcome)>,
    pub from_cache: u32,
    /// Checks run at least once.
    pub ran: u32,
    pub failures: Vec<Failure>,
    pub builds: Vec<Built>,
    /// Products whose build failed, with the build's output; their checks are incomplete.
    pub build_failures: Vec<(String, PathBuf)>,
    /// Families whose `list` failed every try, with the last output.
    pub incomplete_families: Vec<(String, PathBuf)>,
    /// Seconds the check commands ran, builds excluded.
    pub wall_s: f64,
}

impl CheckRun {
    /// Every check passed (or was flaky), and every family and build could be used.
    pub fn passed(&self) -> bool {
        self.build_failures.is_empty()
            && self.incomplete_families.is_empty()
            && self
                .outcomes
                .iter()
                .all(|(_, o)| matches!(o, Outcome::Pass | Outcome::Flaky))
    }

    fn count(&self, outcome: Outcome) -> usize {
        self.outcomes.iter().filter(|(_, o)| *o == outcome).count()
    }

    fn finish(&mut self, check: &CheckName, outcome: Outcome, failure: Option<Failure>) {
        self.outcomes.push((check.clone(), outcome));
        self.failures.extend(failure);
    }
}

/// A check's verdict: the check, its outcome, what to report and the output a failure keeps.
type Final = (Keyed, Outcome, Option<Failure>, Option<PathBuf>);

/// A listed check with its input hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keyed {
    pub listed: Listed,
    pub key: ResultKey,
    /// The paths the hash covers, with their git object IDs.
    pub inputs: Vec<(String, Oid)>,
}

/// A build tree opened for one command.
pub(crate) struct OpenTree {
    pub id: TreeId,
    pub path: PathBuf,
    pub tracked: Vec<String>,
    pub graph: CrateGraph,
}

/// One family's checks, keyed.
pub(crate) struct Prepared {
    pub family: String,
    pub keyed: Vec<Keyed>,
    /// Listed checks whose inputs could not be resolved, which are incomplete if run.
    pub broken: Vec<Failure>,
}

impl Prepared {
    /// Keeps only the checks `wanted` accepts, and finishes the broken ones among them in `run`.
    pub(crate) fn keep(&mut self, wanted: impl Fn(&CheckName) -> bool, run: &mut CheckRun) {
        self.keyed.retain(|k| wanted(&k.listed.check));
        for failure in self.broken.drain(..).filter(|f| wanted(&f.check)) {
            let check = failure.check.clone();
            run.finish(&check, Outcome::Incomplete, Some(failure));
        }
    }
}

/// Told of every cache hit the runner answers a check with, and the commit of the tree it was
/// used in. The idle work's cache audits rerun a sample of these.
pub trait HitSink: Send + Sync {
    fn hit(&self, cached: &CachedResult, commit: &Commit);
}

/// Runs checks in build trees. See the module documentation.
pub struct Checks {
    settings: CheckSettings,
    cache: ResultCache,
    trees: Box<dyn TreeSource>,
    history: Box<dyn CheckHistory>,
    /// `nextest` listings by family and the hash of everything the family can read.
    listings: Mutex<HashMap<(String, InputHash), Vec<Listed>>>,
    next_file: AtomicU64,
    hits: OnceLock<Arc<dyn HitSink>>,
}

impl Checks {
    pub fn new(
        settings: CheckSettings,
        cache: ResultCache,
        trees: Box<dyn TreeSource>,
        history: Box<dyn CheckHistory>,
    ) -> Self {
        Self {
            settings,
            cache,
            trees,
            history,
            listings: Mutex::new(HashMap::new()),
            next_file: AtomicU64::new(0),
            hits: OnceLock::new(),
        }
    }

    /// Tells `sink` of every cache hit from now on. Set after construction, because the idle
    /// work that listens also reruns checks through this runner. Only the first call counts.
    pub fn set_hit_sink(&self, sink: Arc<dyn HitSink>) {
        let _ = self.hits.set(sink);
    }

    pub fn cache(&self) -> &ResultCache {
        &self.cache
    }

    /// Runs the named checks in `tree`, reusing cached results unless `options.rerun`. A name
    /// its family does not list is incomplete.
    pub fn run(
        &self,
        ctx: &Context,
        tree: &TreeId,
        names: &[CheckName],
        options: &RunOptions,
    ) -> CheckRun {
        let mut run = CheckRun::default();
        let tree = match self.open(tree) {
            Ok(tree) => tree,
            Err(error) => {
                for name in names {
                    run.finish(name, Outcome::Incomplete, Some(incomplete(name, &error)));
                }
                return run;
            }
        };
        let families: Vec<&str> = {
            let mut families: Vec<&str> = names.iter().map(CheckName::family).collect();
            families.sort_unstable();
            families.dedup();
            families
        };
        let mut prepared = self.prepare(ctx, &tree, &families, options, &mut run);
        for name in names {
            let listed = prepared.iter().any(|p| {
                p.keyed.iter().any(|k| k.listed.check == *name)
                    || p.broken.iter().any(|f| f.check == *name)
            });
            let family_failed = run
                .incomplete_families
                .iter()
                .any(|(f, _)| f == name.family());
            if !listed && !family_failed {
                let error = Error::NotFound(format!("{name} is not listed by its family"));
                run.finish(name, Outcome::Incomplete, Some(incomplete(name, &error)));
            }
        }
        for family in &mut prepared {
            family.keep(|check| names.contains(check), &mut run);
        }
        self.run_prepared(ctx, &tree, prepared, options, &mut run);
        run
    }

    /// Looks up every prepared check in one request, then runs each family's misses.
    pub(crate) fn run_prepared(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        prepared: Vec<Prepared>,
        options: &RunOptions,
        run: &mut CheckRun,
    ) {
        let hits = self.lookup(&prepared, options);
        let mut hits = hits.into_iter();
        for family in prepared {
            let items: Vec<(Keyed, Option<CachedResult>)> = family
                .keyed
                .into_iter()
                .map(|k| (k, hits.next().flatten()))
                .collect();
            self.execute(ctx, tree, &family.family, items, options, run);
        }
    }

    /// The cached result of each prepared check, in order, or nothing when rerunning.
    pub(crate) fn lookup(
        &self,
        prepared: &[Prepared],
        options: &RunOptions,
    ) -> Vec<Option<CachedResult>> {
        let keys: Vec<ResultKey> = prepared
            .iter()
            .flat_map(|p| p.keyed.iter().map(|k| k.key.clone()))
            .collect();
        if options.rerun {
            vec![None; keys.len()]
        } else {
            self.cache.lookup(&keys)
        }
    }

    pub(crate) fn open(&self, tree: &TreeId) -> Result<OpenTree> {
        let path = self
            .trees
            .path(tree)
            .ok_or_else(|| Error::NotFound(format!("tree {tree}")))?;
        let tracked = self.trees.tracked(&path)?;
        let graph = self.trees.crate_graph(&path, &tracked)?;
        Ok(OpenTree {
            id: tree.clone(),
            path,
            tracked,
            graph,
        })
    }

    /// The families this machine runs, by name.
    pub(crate) fn families(&self) -> impl Iterator<Item = (&str, &Family)> {
        self.settings
            .manifest
            .families
            .iter()
            .filter(|(name, _)| {
                self.settings
                    .families
                    .as_ref()
                    .is_none_or(|list| list.contains(name))
            })
            .filter(|(_, declared)| match &declared.family.platforms {
                Platforms::Any(_) => true,
                Platforms::Each(list) => list.contains(&self.settings.os),
            })
            .map(|(name, declared)| (name.as_str(), &declared.family))
    }

    /// Lists and keys the checks of each family named. A family whose listing fails is recorded
    /// in `run` as incomplete; a check whose inputs cannot be resolved is kept in `broken`.
    pub(crate) fn prepare(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        families: &[&str],
        options: &RunOptions,
        run: &mut CheckRun,
    ) -> Vec<Prepared> {
        let mut prepared = Vec::new();
        for (name, family) in self.families().filter(|(n, _)| families.contains(n)) {
            let listed = match self.list(ctx, tree, name, family, options, run) {
                Ok(listed) => listed,
                Err(output) => {
                    run.incomplete_families.push((name.to_owned(), output));
                    continue;
                }
            };
            let mut keyed = Vec::new();
            let mut broken = Vec::new();
            for listed in listed {
                match self.key(ctx, tree, family, listed.clone()) {
                    Ok(k) => keyed.push(k),
                    Err(error) => broken.push(incomplete(&listed.check, &error)),
                }
            }
            prepared.push(Prepared {
                family: name.to_owned(),
                keyed,
                broken,
            });
        }
        prepared
    }

    /// The family's checks. `Err` holds the output of the last failed try.
    fn list(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        name: &str,
        family: &Family,
        options: &RunOptions,
        run: &mut CheckRun,
    ) -> std::result::Result<Vec<Listed>, PathBuf> {
        let memo = match family.adapter {
            Adapter::Jsonl => None,
            Adapter::Nextest => {
                let hash = self
                    .listing_hash(ctx, tree, family)
                    .map_err(|_| PathBuf::new())?;
                let memo = (name.to_owned(), hash);
                let listings = self.listings.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(listed) = listings.get(&memo) {
                    return Ok(listed.clone());
                }
                drop(listings);
                Self::build(ctx, tree, family, &[], run)?;
                Some(memo)
            }
        };
        let (args, cwd) = match family.adapter {
            Adapter::Jsonl => (
                adapters::words(family.list.as_deref().unwrap_or_default()),
                tree.path.clone(),
            ),
            Adapter::Nextest => (
                adapters::nextest_list_command(family.list.as_deref()),
                self.nextest_dir(tree, family),
            ),
        };
        let mut output = PathBuf::new();
        for _ in 0..TRIES {
            let result = self.launch(ctx, tree, name, family, args.clone(), &cwd, vec![], options);
            output.clone_from(&result.output);
            if result.exit != Some(0) {
                continue;
            }
            let Ok(text) = fs::read_to_string(&result.output) else {
                continue;
            };
            let listed = match family.adapter {
                Adapter::Jsonl => adapters::jsonl_list(&text, name),
                Adapter::Nextest => adapters::nextest_list(&text, name),
            };
            if let Some(memo) = memo {
                let mut listings = self.listings.lock().unwrap_or_else(PoisonError::into_inner);
                listings.insert(memo, listed.clone());
            }
            return Ok(listed);
        }
        Err(output)
    }

    /// The hash of everything a `nextest` family can read: its inputs with `crate-closure`
    /// standing for every workspace package. Its tests cannot change while this holds.
    fn listing_hash(&self, ctx: &Context, tree: &OpenTree, family: &Family) -> Result<InputHash> {
        let patterns: Vec<InputPattern> = family
            .inputs
            .iter()
            .flat_map(|pattern| match pattern {
                InputPattern::CrateClosure => tree
                    .graph
                    .packages
                    .iter()
                    .map(|p| InputPattern::Crate(p.name.clone()))
                    .collect(),
                other => vec![other.clone()],
            })
            .collect();
        let paths = inputs::resolve(&patterns, None, &tree.graph, &tree.tracked)
            .map_err(|e| Error::Invalid(e.to_string()))?;
        let ids = inputs::object_ids(ctx.git.as_ref(), &tree.path, &paths)?;
        Ok(self.hash(family, &ids))
    }

    fn hash(&self, family: &Family, ids: &[(String, Oid)]) -> InputHash {
        let commands = hash::commands(family, &self.settings.manifest);
        hash::input_hash(&HashParts {
            inputs: ids,
            commands: &commands,
            toolchain: &self.settings.toolchain,
            platform: hash::platform(&family.platforms, self.settings.os),
        })
    }

    /// The check's input hash over its family's inputs and its own.
    pub(crate) fn key(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        family: &Family,
        listed: Listed,
    ) -> Result<Keyed> {
        let mut patterns = family.inputs.clone();
        patterns.extend(listed.inputs.iter().cloned().map(InputPattern::from));
        let paths = inputs::resolve(
            &patterns,
            listed.package.as_deref(),
            &tree.graph,
            &tree.tracked,
        )
        .map_err(|e| Error::Invalid(e.to_string()))?;
        let ids = inputs::object_ids(ctx.git.as_ref(), &tree.path, &paths)?;
        let key = ResultKey {
            check: listed.check.clone(),
            hash: self.hash(family, &ids),
            platform: hash::platform(&family.platforms, self.settings.os),
        };
        Ok(Keyed {
            listed,
            key,
            inputs: ids,
        })
    }
}

fn failure(report: &Reported, output: &Path) -> Failure {
    Failure {
        check: report.check.clone(),
        outcome: Outcome::Fail,
        at: report.at.clone(),
        message: report.message.clone(),
        output: Some(output.display().to_string()),
    }
}

fn incomplete(check: &CheckName, error: &Error) -> Failure {
    Failure {
        check: check.clone(),
        outcome: Outcome::Incomplete,
        at: None,
        message: Some(error.to_string()),
        output: None,
    }
}

fn path_text(path: &Path) -> String {
    path.display().to_string()
}
