//! The check runner: lists a family's checks, computes each one's input hash, looks them up in
//! the result cache, builds what the rest need and runs them with the family's adapter (see
//! "Checks" in `docs/ignite.md`).
//!
//! - Listing: a `jsonl` family's `list` command, or cargo-nextest's list for a `nextest` family.
//!   A listing is kept under the hash of everything the family's `list` can read, in memory, in
//!   the local database and at the hub, and is made again only when none of them has one for
//!   the tree's inputs ([`listings`]). cargo-nextest can list tests only after building them, so
//!   a family whose listing is kept and whose tests are all cached builds nothing.
//!   A listing with no checks that exited 0 is not believed: a `nextest` family always lists
//!   tests, and a `jsonl` family that has listed checks before does not suddenly list none, so
//!   either is a failed listing, never kept.
//! - Scope: the landing gate leaves out every family whose inputs the change does not touch,
//!   before it lists or builds anything ([`scope`]).
//! - Failing fast: the landing gate stops at the first fault it knows of. It lists first the
//!   families that build nothing to be listed, while the products the others need build, and
//!   refuses the landing before any other family is listed when a listed check's manifest
//!   entry is invalid ([`prepare`]). It starts every family at once, cheapest per expected
//!   failure first, and starts no further command in any family once a failure is confirmed by
//!   its rerun ([`gate`]). What it leaves out is counted in `CheckRun::unlisted` and `skipped`.
//! - Lookup: one result cache request per command, for every check it needs ([`ResultCache`]).
//! - A failed check runs a second time at once. A pass then makes it flaky, which passes and is
//!   reported to the hub's flaky table. A check that a run did not report, or whose family's
//!   `list` failed, is `incomplete` and is tried at most twice more. Incomplete and flaky
//!   outcomes are never cached.
//! - A check whose command says it cannot run on this machine, and every check a command left
//!   unreported when it ran past its hang limit, is `incomplete` at once and not run again.
//!   The hang limit is four times the command's slowest pass in 30 days on this machine, at
//!   least 15 minutes and at most 40, and 30 minutes with no pass on record.
//! - A `jsonl` command that hung, or ended by a signal or an error no failing check explains,
//!   may have been cut in the middle of a check's steps: the check of its last line is run
//!   again alone instead of counting as a pass.
//! - A cached failure from another machine is not trusted: the check runs here, and a pass is
//!   stored, which replaces the failure and makes the hub file a ticket for that machine.
//! - The output of a failed, flaky or incomplete run is kept for 14 days, other output for a
//!   day ([`crate::supervise::keep`]).
//! - Every command leaves a record with its cost, and a family answered wholly from the cache
//!   leaves one record with the verb `cached` and no cost.
//!
//! The verbs `ignite test`, `ignite check` and `ignite why` are in [`verbs`].

pub mod adapters;
mod batches;
mod execute;
mod express;
mod gate;
mod keying;
pub(crate) mod listings;
mod observed;
pub(crate) mod plan;
mod prepare;
mod products;
mod regression;
pub(crate) mod run_reads;
mod scope;
pub mod sources;
mod spent;
mod split;
mod time_lines;
mod units;
mod unstarted;

pub use units::ListingTime;
pub(crate) mod verbs;
mod why_record;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use ignite_core::config::Os;
use ignite_core::hash::{self, HashParts};
use ignite_core::ids::{CheckName, Commit, InputHash, Oid, SessionId, TicketId, TreeId};
use ignite_core::inputs::CrateGraph;
use ignite_core::manifest::{Family, Manifest, Platforms};
use ignite_core::protocol::hub::IncompleteReason;
use ignite_core::protocol::hub::{CachedResult, ListingKey, ResultKey};
use ignite_core::protocol::{Class, Outcome};
use ignite_core::traits::{Error, Result};

use crate::cache::ResultCache;
use crate::server::Context;
use listings::{Kept, Listing};

pub use adapters::{Listed, Reported, Target};
pub use gate::{Carried, Excused, GateBudget, GateRun};
pub use plan::Scoring;
pub use sources::{CheckHistory, HubHistory, TreeSource};
pub use verbs::{CheckVerb, TestVerb, WhyVerb};

/// How many times one check or one family listing is tried while it comes back incomplete.
const TRIES: u32 = 3;

/// The bounds of a command's hang limit, and the limit of a command with no pass on record,
/// in seconds.
const HANG_MIN_S: f64 = 15.0 * 60.0;
const HANG_MAX_S: f64 = 40.0 * 60.0;
const HANG_UNKNOWN_S: u64 = 30 * 60;

/// A command's hang limit: four times its slowest pass, within the bounds above.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn hang_limit_s(slowest_pass_s: Option<f64>) -> u64 {
    slowest_pass_s.map_or(HANG_UNKNOWN_S, |slowest| {
        (4.0 * slowest).clamp(HANG_MIN_S, HANG_MAX_S) as u64
    })
}

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
    /// The `ignite` executable that the compiler wrapper and test runner scripts call. Empty
    /// when this daemon cannot name one: Rust test runs then record no per-process units.
    pub ignite_exe: PathBuf,
    /// The directory of the compiler wrapper and test runner scripts (`crate::wrap`).
    pub shims: PathBuf,
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
    /// Why an `Incomplete` check has no verdict, when a run left it so.
    pub reason: Option<IncompleteReason>,
}

/// A build product that had to be built.
#[derive(Clone, Debug, PartialEq)]
pub struct Built {
    pub product: String,
    pub wall_s: f64,
    /// Distinct crates the build compiled, or `None` when the build recorded no compiler
    /// processes although it used CPU time (it ran without the compiler wrapper).
    pub rebuilt: Option<usize>,
    /// The crates the build spent the most CPU seconds compiling, most first, with their
    /// seconds (see [`crate::build::costliest`]).
    pub costliest: Vec<(String, f64)>,
    /// CPU seconds of the build and its descendants.
    pub cpu_s: f64,
    /// Seconds the build waited for cores, apart from `wall_s`.
    pub queued_s: f64,
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
    /// Families the landing gate left out, because the change touches none of their inputs.
    pub out_of_scope: Vec<String>,
    /// Checks the landing gate chose but neither ran nor took from the cache, because it
    /// stopped at the first fault it knew of.
    pub skipped: u32,
    /// Families in the landing gate's scope that it did not list, because it stopped at the
    /// first fault it knew of.
    pub unlisted: Vec<String>,
    /// Families in the landing gate's scope that it left to tip verification without listing
    /// them, because listing them needs a product the tree has never built and no listing of
    /// them is kept.
    pub cold_families: Vec<String>,
    /// Seconds the check commands ran, builds excluded.
    pub wall_s: f64,
    /// Each `list` command run for a family no kept listing served, in the order they ran.
    pub listings: Vec<ListingTime>,
    /// Families whose checks came from a kept listing, without running their `list` command.
    pub listings_kept: u32,
    /// For each check a run gave a verdict, the hub's ID for that run's record, when the hub
    /// took it.
    pub records: Vec<(CheckName, ignite_core::ids::RecordId)>,
    /// The command's stages, which run one after another, with their seconds (the `took` line).
    pub stages: Vec<(String, f64)>,
    /// Steps of at least a second inside the stages, with their seconds (the `slow` line).
    pub slow_steps: Vec<(String, f64)>,
}

impl CheckRun {
    /// Every check passed (or was flaky), and every family and build could be used.
    pub fn passed(&self) -> bool {
        self.build_failures.is_empty()
            && self.incomplete_families.is_empty()
            && self.skipped == 0
            && self.unlisted.is_empty()
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

/// A check's verdict: the check, its outcome, what to report, the output a failure keeps and
/// what its runs cost.
type Final = (
    Keyed,
    Outcome,
    Option<Failure>,
    Option<PathBuf>,
    ignite_core::protocol::hub::ResultCost,
    // The hub's ID for the record of the run whose output the outcome carries, if it took it.
    Option<ignite_core::ids::RecordId>,
);

/// Why a family's `list` gave no checks.
#[derive(Clone)]
enum ListError {
    /// A product it needs failed to build, which `CheckRun::build_failures` records.
    Build,
    /// The command failed every try; holds the last output.
    Output(PathBuf),
}

/// What listing a family this machine does not run came to.
#[derive(Clone)]
enum Foreign {
    Listed(Listing),
    /// The failure, and the builds that failed on the way, for `CheckRun::build_failures`.
    Failed(ListError, Vec<(String, PathBuf)>),
}

/// A listed check with its input hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keyed {
    pub listed: Listed,
    pub key: ResultKey,
    /// The paths the hash covers, with their git object IDs.
    pub inputs: Vec<(String, Oid)>,
    /// What the hash knows of the files the check opens at run time.
    pub reads: run_reads::RunReads,
}

/// A build tree opened for one command.
pub(crate) struct OpenTree {
    pub id: TreeId,
    pub path: PathBuf,
    pub tracked: Vec<String>,
    pub graph: CrateGraph,
}

/// The message of a check that a build it needs kept from running.
pub(crate) const BUILD_FAILED: &str = "a build it needs failed";

/// The words in an unlisted check's message (`the inputs of <check> cannot be resolved: <why>`)
/// before what is wrong with its manifest entry. Tip verification reports the checks with one
/// family and message as one cause.
pub(crate) const UNRESOLVED: &str = " cannot be resolved: ";

/// The words in an unlisted family's message (`a product the listing of <family> needs failed
/// to build: <products>`) before the products, separated by `, `.
pub(crate) const LISTING_BUILD_FAILED: &str = " needs failed to build: ";

/// One family's checks, keyed.
#[derive(Clone)]
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
///
/// A runner opened on a tree ([`Checks::open`]) is a view of this one with the families and
/// products of the tree's own manifests (see `crate::manifests`), sharing everything else.
pub struct Checks {
    settings: CheckSettings,
    shared: Arc<Shared>,
    /// The builds this view's command was answered with, by product and usage. Families on
    /// threads of their own that ask for one product share its build, and each is answered
    /// with the same result; only the first reports and records it.
    builds_seen: Mutex<HashSet<(String, u64, u64)>>,
    /// The read lists this view's runs recorded, by check, until their results are stored.
    fresh_reads: Mutex<HashMap<CheckName, Vec<String>>>,
}

/// What every view of one runner shares: the cache, the trees, the history and what the
/// runner remembers between commands. [`Checks`] dereferences to it.
pub struct Shared {
    cache: ResultCache,
    trees: Box<dyn TreeSource>,
    history: Box<dyn CheckHistory>,
    /// Kept listings by family and listing hash, as this daemon has made or read them.
    listings: Mutex<HashMap<ListingKey, Arc<Kept>>>,
    /// Listings made here that must not be kept (see [`listings`]), which nobody else has
    /// either, so they are not asked for.
    not_kept: Mutex<HashSet<ListingKey>>,
    /// The `jsonl` families that have listed at least one check since the daemon started. A
    /// listing that later comes back empty is taken for a broken `list` command.
    listed_before: Mutex<HashSet<String>>,
    /// What listing each family this machine does not run last came to, by family, with the
    /// hash of the inputs it was for. Tip verification lists such families on every round only
    /// to key the checks other machines run, so a listing that already failed (or succeeded) for
    /// the same inputs is not tried again.
    foreign_listings: Mutex<HashMap<String, (InputHash, Foreign)>>,
    next_file: AtomicU64,
    hits: OnceLock<Arc<dyn HitSink>>,
}

impl std::ops::Deref for Checks {
    type Target = Shared;

    fn deref(&self) -> &Shared {
        &self.shared
    }
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
            shared: Arc::new(Shared {
                cache,
                trees,
                history,
                listings: Mutex::new(HashMap::new()),
                not_kept: Mutex::new(HashSet::new()),
                listed_before: Mutex::new(HashSet::new()),
                foreign_listings: Mutex::new(HashMap::new()),
                next_file: AtomicU64::new(0),
                hits: OnceLock::new(),
            }),
            builds_seen: Mutex::default(),
            fresh_reads: Mutex::default(),
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
        let (checks, tree) = match self.open(ctx, tree) {
            Ok(opened) => opened,
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
        let mut prepared = checks.prepare(ctx, &tree, &families, options, &mut run);
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
        checks.run_prepared(ctx, &tree, prepared, options, &mut run);
        run
    }

    /// The cached result of each prepared check, in order, or nothing when rerunning.
    pub(crate) fn lookup(
        &self,
        prepared: &[Prepared],
        options: &RunOptions,
    ) -> Vec<Option<CachedResult>> {
        let keyed: Vec<&Keyed> = prepared.iter().flat_map(|p| &p.keyed).collect();
        if options.rerun {
            return vec![None; keyed.len()];
        }
        // A check whose hash may not cover what it opens at run time is not looked up.
        let keys: Vec<ResultKey> = keyed
            .iter()
            .filter(|k| k.reads.may_hit())
            .map(|k| k.key.clone())
            .collect();
        let mut found = self.cache.lookup(&keys).into_iter();
        keyed
            .iter()
            .map(|k| {
                if k.reads.may_hit() {
                    found.next().flatten()
                } else {
                    None
                }
            })
            .collect()
    }

    /// Opens `tree` for one command, with this runner as it runs there: a view whose families
    /// and products are those of the tree's own manifests, in `repository.checks` within it.
    /// Every command on the tree goes through the view, so the commands it lists, builds and
    /// runs are the tree's, whatever manifests this runner was made with; those stand only for
    /// a tree with no manifest directory. Manifests the tree has but that cannot be parsed are
    /// an error.
    pub(crate) fn open(&self, ctx: &Context, tree: &TreeId) -> Result<(Checks, OpenTree)> {
        let path = self
            .trees
            .path(tree)
            .ok_or_else(|| Error::NotFound(format!("tree {tree}")))?;
        let own = crate::manifests::in_tree(&path, &ctx.config.repository.checks)
            .map_err(|e| Error::Invalid(format!("the manifests of tree {tree}: {e}")))?;
        let tracked = self.trees.tracked(&path)?;
        let graph = self.trees.crate_graph(&path, &tracked)?;
        let mut settings = self.settings.clone();
        if let Some(own) = own {
            settings.manifest = own;
        }
        let view = Checks {
            settings,
            shared: self.shared.clone(),
            builds_seen: Mutex::default(),
            fresh_reads: Mutex::default(),
        };
        let tree = OpenTree {
            id: tree.clone(),
            path,
            tracked,
            graph,
        };
        Ok((view, tree))
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

    /// [`Checks::list`], except that for a family this machine does not run the outcome is kept
    /// for the family's current inputs, and a later call with the same inputs gets it back
    /// without building or running anything.
    #[allow(clippy::too_many_arguments)]
    fn list_remembering_foreign(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        (name, family): (&str, &Family),
        options: &RunOptions,
        run: &mut CheckRun,
        key: Option<ListingKey>,
        kept: Option<(ListingKey, Arc<Kept>)>,
    ) -> std::result::Result<Listing, ListError> {
        let foreign = !self.families().any(|(own, _)| own == name);
        let hash = key.as_ref().filter(|_| foreign).map(|k| k.hash.clone());
        let Some(hash) = hash else {
            return self.list(ctx, tree, name, family, options, run, key, kept);
        };
        let remembered = self
            .foreign_listings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(name)
            .filter(|(seen, _)| *seen == hash)
            .map(|(_, outcome)| outcome.clone());
        if let Some(outcome) = remembered {
            return match outcome {
                Foreign::Listed(listing) => Ok(listing),
                Foreign::Failed(failure, builds) => {
                    run.build_failures.extend(builds);
                    Err(failure)
                }
            };
        }
        let before = run.build_failures.len();
        let result = self.list(ctx, tree, name, family, options, run, key, kept);
        let outcome = match &result {
            Ok(listing) => Foreign::Listed(listing.clone()),
            Err(failure) => Foreign::Failed(failure.clone(), run.build_failures[before..].to_vec()),
        };
        self.foreign_listings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(name.to_owned(), (hash, outcome));
        result
    }

    /// Forgets what listing a family this machine does not run came to.
    fn forget_foreign(&self, name: &str) {
        let mut foreign = self
            .foreign_listings
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        foreign.remove(name);
    }

    fn hash(&self, family: &Family, ids: &[(String, Oid)], digest: Option<&str>) -> InputHash {
        let commands = hash::commands(family, &self.settings.manifest);
        hash::input_hash(&HashParts {
            inputs: ids,
            digest,
            commands: &commands,
            toolchain: &self.settings.toolchain,
            platform: hash::platform(&family.platforms, self.settings.os),
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
        reason: None,
    }
}

fn incomplete(check: &CheckName, error: &Error) -> Failure {
    Failure {
        check: check.clone(),
        outcome: Outcome::Incomplete,
        at: None,
        message: Some(error.to_string()),
        output: None,
        reason: None,
    }
}

/// The failure of a check whose manifest entry is invalid: its input patterns cannot be
/// resolved, or its listing was refused.
fn invalid_entry(check: &CheckName, error: &Error) -> Failure {
    Failure {
        reason: Some(IncompleteReason::InvalidEntry),
        ..incomplete(check, error)
    }
}

/// `IGNITE_BASE`, the commit staging is at, for a family's `list` and `run` commands. A check
/// that reads the commits since then (the commit subjects) prints a digest of them when listed.
pub(crate) fn base_env(ctx: &Context) -> Vec<(String, String)> {
    ctx.git
        .tip(&ctx.config.repository.staging)
        .map(|base| vec![("IGNITE_BASE".to_owned(), base.to_string())])
        .unwrap_or_default()
}

fn path_text(path: &Path) -> String {
    path.display().to_string()
}
