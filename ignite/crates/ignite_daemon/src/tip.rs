//! Tip verification: once per `staging` tip, run the regression checks the gate left out, move
//! `master` forward when they all pass, and on a failure find the landing that caused it and
//! report a regression (see "Tip verification" and "Finding the culprit" in `docs/ignite.md`).
//!
//! - A round starts when the hub's change feed shows something new. The first machine selects
//!   the newest `staging` commit at the hub. Peers verify the same candidate while its required
//!   checks are pending. After it verifies, fails, or has a blocker a new landing could fix, the
//!   next round can select the newest tip.
//! - The set: every regression check of every regression family that the hub has not switched
//!   off, with no cached result for the tip. A check whose inputs did not change since the last
//!   verified commit has the same input hash, so the result cache answers it. A check this
//!   machine cannot run is in the set as a key that only another machine's pass satisfies, and
//!   a family that cannot be listed here makes the tip wait ([`TipSet::unlisted`]).
//! - A check of a family whose `covered_by` names another family is left out when the covering
//!   family lists a check of the same name: the covering check stands for it (see `covered`).
//! - The set is claimed from the hub in batches of one family's checks, so two machines never
//!   run the same batch. A batch holds about a minute of recorded cost, cut alike on every
//!   machine from costs the hub fixes for the tip. Checks only this machine's platform can run
//!   go first. Each family is listed once and several families run at once (see `families`).
//! - Each round sends the hub a record of where its time went (see `meter`).
//! - With `repository.green` set, the build checks run before any other, and `green` moves to
//!   the tip once they pass (see `green`).
//! - `master` moves to the tip once every check in its set has a pass on each platform it
//!   needs. A failure or an `incomplete` check holds it. A cached failure is run once here
//!   before it holds `master`, so a fault of one run, or of Ignite, heals.
//! - A check that stays `incomplete` through its attempts is reported to the hub once, which
//!   files a ticket, and is not run again by this machine for an hour, then two, and so on up
//!   to a day. The hold also leaves its batch unclaimed, so the claim lapses and another
//!   machine can take it. `master` keeps waiting for the check.
//! - Nothing a round meets sticks silently: a family that cannot be listed, a check whose inputs
//!   cannot be resolved and a push that moves `master` but is refused are reported to the hub
//!   once per tip, and the tip waits. Telemetry the hub refuses is logged and never fails a
//!   round.
//! - A round that has run a batch, or is building or listing a family, is finished even when a
//!   newer tip arrives, so landings that keep arriving cannot keep every tip from being
//!   verified. An idle round leaves for a newer tip only after its target is no longer the
//!   shared candidate.
//! - A failure's candidates are the landings since the last verified commit that changed one
//!   of the check's inputs. One candidate is the culprit with nothing run; several are searched
//!   by running that check between them, at most [`MAX_RUNS`] times. A landing is one
//!   ticket's commits, as the hub recorded them when the ticket landed, so a report names the
//!   ticket whose landing caused the failure.
//! - With `[revert] broken_build` set, a failing build check whose search ends at one landing
//!   reverts that landing instead of reporting a regression, when the conditions in `revert`
//!   hold.
//! - When the tips since the last verified commit have waited [`MACHINE_WAIT_S`] for checks
//!   only another platform's machines can run, the hub is told once per tip and platform, and
//!   files a P1 ticket naming those machines. `master` keeps waiting. When no machine of that
//!   platform declared in `ignite.toml` runs a waited check's family at all, the hub is told at
//!   once, and the round's outcome names the family.

mod cache;
mod candidate;
mod coverage;
mod covered;
mod culprit;
mod families;
mod green;
mod machines;
mod meter;
mod report;
mod revert;
mod round;
mod split;
mod state;
mod verify;
mod work;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::sync::Arc;

use ignite_core::config::Os;
use ignite_core::ids::{CheckName, Commit, Seq, TicketId};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{IncompleteReason, ResultKey};
use ignite_core::time::Timestamp;
use ignite_core::tipround::CulpritSearch;
use ignite_core::traits::Result;

use crate::checks::Scoring;
use crate::server::Context;

pub(crate) use culprit::{bisect, with_culprit};
pub use round::{ROUND_VERB, TipLoop, TipRoundVerb};
pub use work::{CheckTipWork, ticketed_landings};

/// The most runs one culprit search may make before it reports a range.
pub const MAX_RUNS: u32 = 10;

/// How long tips may wait for checks only another platform's machines can run before the hub
/// files a ticket naming those machines.
pub const MACHINE_WAIT_S: f64 = 3_600.0;

/// How long a tip that is waiting is left before it is verified again, unless a newer tip
/// arrives. Each round lists every family, and the feed changes every minute or two, so a tip
/// held by incomplete checks was listed again and again for nothing.
pub const WAITING_RETRY_S: f64 = 600.0;

/// How many lines of a failure's output a regression report carries.
const OUTPUT_LINES: usize = 40;

/// One regression check that a tip needs a pass for, on one platform.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TipCheck {
    pub check: CheckName,
    pub family: String,
    /// The result cache key at the commit it was listed at.
    pub key: ResultKey,
    /// The paths its input hash covers, relative to the repository.
    pub inputs: Vec<String>,
    /// False for a key on another platform, which only another machine can pass.
    pub runnable: bool,
    /// False when the hub no longer trusts the check's cached results, so it runs every time.
    pub reuse: bool,
    /// The check's calibration and results belong to the verifier that listed it.
    pub machine_relative: bool,
}

/// One check's outcome from a run at one commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TipRun {
    pub check: CheckName,
    /// The key of the calibration the run used, for checks that depend on it.
    pub measurement_hash: Option<ignite_core::ids::InputHash>,
    pub outcome: Outcome,
    /// The file or reference that holds a failure's output.
    pub output: Option<String>,
    /// Why an `Incomplete` check has no verdict, when the command's own run left it so. `None`
    /// when the fault was not the check's: no landing tree was free, for one.
    pub reason: Option<IncompleteReason>,
    /// The product whose failed build kept the check from running. The round reports the build
    /// once, not each of its checks.
    pub build: Option<String>,
}

/// A regression check or family that tip verification cannot list or key on this machine. The
/// tip waits for it, and the hub is told once unless the machine could never run it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unlisted {
    /// The check that cannot be keyed, or `<family>:listing` for a whole family.
    pub name: CheckName,
    pub family: String,
    /// What went wrong, for the report.
    pub message: String,
    /// False for a family this machine does not run: its machine's trouble is not a fault of the
    /// repository, so a machine that can run it moves `master` instead of a ticket.
    pub reportable: bool,
    /// The required platforms when a family must pass separately on each of them.
    pub platforms: Vec<Os>,
}

/// What a tip needs: the keyed checks, and what could not be listed or keyed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TipSet {
    pub checks: Vec<TipCheck>,
    pub unlisted: Vec<Unlisted>,
    /// The checks this machine could run that the hub has switched off as flaky. The tip
    /// needs none of them, but batches are cut over them too: the hub can switch a check off
    /// partway through a round, and a machine that listed the family after that must still
    /// cut the same batches as one that listed it before.
    pub off: Vec<TipCheck>,
}

/// A check that batches are cut over: every check this machine can run that the family
/// listed at the tip, whether the tip needs it or not.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Listed {
    family: String,
    check: CheckName,
    /// The check passes on any platform, so its family is not one only this platform runs.
    portable: bool,
}

/// One landing: one ticket's commits, oldest first, and the paths they changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TipLanding {
    /// The ticket that landed it, when known.
    pub ticket: Option<TicketId>,
    pub commits: Vec<Commit>,
    pub paths: Vec<String>,
}

impl TipLanding {
    fn last(&self) -> Option<&Commit> {
        self.commits.last()
    }
}

/// What tip verification needs beyond [`Context`]: the checks at a commit, a way to run them
/// there and the landings between two commits. [`CheckTipWork`] is the real one.
pub trait TipWork: Send + Sync {
    /// Every regression check `commit` needs a pass for, as this machine can list them.
    fn checks(&self, ctx: &Context, commit: &Commit) -> Result<TipSet>;
    /// Runs the named checks at `commit` in a landing tree, in the `tip` class, storing their
    /// results. `rerun` ignores cached results.
    fn run(&self, ctx: &Context, commit: &Commit, checks: &[CheckName], rerun: bool)
    -> Vec<TipRun>;
    /// Runs one family's batches at `commit`, listing the family once. `claim` is asked right
    /// before each batch, with its number, and a batch it refuses is skipped. Returns the runs
    /// and how many batches ran.
    fn run_batches(
        &self,
        ctx: &Context,
        commit: &Commit,
        family: &str,
        batches: &[(u32, Vec<CheckName>)],
        claim: &dyn Fn(u32) -> bool,
    ) -> (Vec<TipRun>, usize) {
        let _ = family;
        let mut runs = Vec::new();
        let mut ran = 0;
        for (batch, names) in batches {
            if claim(*batch) {
                ran += 1;
                runs.extend(self.run(ctx, commit, names, false));
            }
        }
        (runs, ran)
    }
    /// How many families a round may run at once.
    fn parallel(&self) -> usize {
        1
    }
    /// Each family's `covered_by`: the family whose checks stand for its checks of the same
    /// name.
    fn covered_by(&self) -> BTreeMap<String, String> {
        BTreeMap::new()
    }
    /// The families declared `builds = true`, whose checks fail when the code does not compile
    /// or type-check. The work runs their checks with the cores reserved for them when
    /// `repository.green` is set.
    fn build_families(&self) -> std::collections::BTreeSet<String> {
        std::collections::BTreeSet::new()
    }
    /// The landings after `from` up to and including `to`, oldest first.
    fn landings(&self, ctx: &Context, from: &Commit, to: &Commit) -> Result<Vec<TipLanding>>;
    /// What `checks` at `to` are scored by for the tip budget, with the landings since `from`;
    /// `None` when it cannot be known, and then every check runs before `master` moves.
    fn scoring(
        &self,
        ctx: &Context,
        from: &Commit,
        to: &Commit,
        checks: &[&TipCheck],
    ) -> Option<Scoring> {
        let _ = (ctx, from, to, checks);
        None
    }
}

/// How one tip's verification ended. A tip round's verb process answers with it (see `round`).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TipOutcome {
    /// The required checks passed and `master` is at the tip.
    Verified,
    /// Master moved while platforms with no enabled machine still needed checks.
    VerifiedWithDeferrals {
        checks: BTreeMap<Os, Vec<CheckName>>,
    },
    /// These checks have no result yet: another machine holds their batch, only another
    /// platform can run them, or their run was incomplete. `master` waits.
    Waiting {
        checks: Vec<CheckName>,
        /// One line for each platform and family the tip waits on that no machine declared in
        /// `ignite.toml` can run there, so it would wait forever. Empty when every waited check
        /// has a machine.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        unrunnable: Vec<String>,
    },
    /// These checks failed at the tip. `master` stays where it is.
    Failed { checks: Vec<CheckName> },
    /// A newer tip arrived during the round; the next round verifies it.
    Superseded { by: Commit },
}

/// Where a culprit search ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Culprit {
    /// The landings that may have caused the failure: one when the search finished, several
    /// when it ended early with a range.
    pub suspects: Vec<TipLanding>,
    /// The last commit of the last suspect, where the check is known to fail.
    pub first_bad: Commit,
    /// The newest commit known to pass: the one before the first suspect.
    pub last_good: Commit,
    /// How many times the check ran during the search.
    pub runs: u32,
    /// How the search ended, which the regression ticket states.
    pub search: CulpritSearch,
}

/// Verifies tips one round at a time. The daemon keeps one and calls [`TipVerifier::round`] in
/// a loop on its own thread.
pub struct TipVerifier {
    work: Arc<dyn TipWork>,
    /// The Ignite commit that records name.
    ignite: String,
    /// This machine's hostname, which the hub's tickets about its checks name. Empty when not
    /// set, and then the machine's ID stands in for it.
    host: String,
    /// The feed position already seen.
    since: Seq,
    /// The last tip that was verified or failed. A waiting or superseded tip is tried again.
    settled: Option<Commit>,
    /// Policy under which the last round started. An operator may make the same tip runnable.
    fleet_version: Option<u64>,
    /// Failures already reported, so a cached failure that stands for later tips is reported
    /// once.
    reported: HashSet<ResultKey>,
    /// The checks whose failure was left unreported once because a newer `staging` tip changed
    /// what they read (see `culprit`). A second failure of one of them is reported whatever
    /// the newer tip changed. Cleared when `master` moves.
    deferred: HashSet<CheckName>,
    /// When the tips since the last verified commit first waited on each other platform. The
    /// wait spans tips, because on a busy day a new tip arrives well within the hour.
    waiting_since: BTreeMap<Os, Timestamp>,
    /// The tips and platforms already reported as waiting.
    flagged: HashSet<(Commit, Os)>,
    /// The output references that runs on this machine returned in this round. Only these are
    /// opened to quote a failure's last lines.
    own_outputs: HashSet<String>,
    /// The checks this round cuts its batches over (see [`Listed`]).
    listed: Vec<Listed>,
    /// When this machine may run a check that gave up again, by its key: an hour after the first
    /// time, then twice as long each time, up to a day.
    held_off: HashMap<ResultKey, Timestamp>,
    /// How many runs of each check ended incomplete, which sets how long it is held off.
    incomplete_runs: HashMap<ResultKey, u32>,
    /// The incomplete checks the hub was told of.
    reported_incomplete: HashSet<ResultKey>,
    /// The tips and names (a family that could not be listed, a check without inputs, a refused
    /// push) the hub was told of.
    reported_stuck: HashSet<(Commit, CheckName)>,
    /// The cached failures run again here once, so that they stand afterwards.
    rechecked: HashSet<ResultKey>,
    /// A tip whose last round ended waiting, and when it may be verified again.
    resting: Option<(Commit, Timestamp)>,
    /// Where the memory above is saved between rounds (see `state`); `None` keeps it in
    /// memory only.
    saved: Option<state::Saved>,
    /// How often a running batch's claim is renewed (see `families`).
    renew_every: std::time::Duration,
    /// The commits of each landing this round reverted, and the commit that undid them (see
    /// `revert`).
    reverted: Vec<(Vec<Commit>, Commit)>,
    /// Reverts pushed that the hub has not taken yet.
    unsent_reverts: Vec<ignite_core::protocol::hub::Reverted>,
    /// How the round waits for a lease another holder has.
    pause: revert::Pause,
}

impl TipVerifier {
    pub fn new(work: Arc<dyn TipWork>, ignite: impl Into<String>) -> Self {
        Self {
            work,
            ignite: ignite.into(),
            host: String::new(),
            since: Seq(0),
            settled: None,
            fleet_version: None,
            reported: HashSet::new(),
            deferred: HashSet::new(),
            waiting_since: BTreeMap::new(),
            flagged: HashSet::new(),
            own_outputs: HashSet::new(),
            listed: Vec::new(),
            held_off: HashMap::new(),
            incomplete_runs: HashMap::new(),
            reported_incomplete: HashSet::new(),
            reported_stuck: HashSet::new(),
            rechecked: HashSet::new(),
            resting: None,
            saved: None,
            renew_every: families::RENEW_EVERY,
            reverted: Vec::new(),
            unsent_reverts: Vec::new(),
            pause: revert::sleep(),
        }
    }

    /// Renews the claims of running batches every `every` instead of every
    /// [`families::RENEW_EVERY`], as tests do.
    #[must_use]
    pub fn renewing_claims_every(mut self, every: std::time::Duration) -> Self {
        self.renew_every = every;
        self
    }

    /// Names this machine's hostname in the reports of checks that stay incomplete, so the
    /// hub's tickets say `jarvis` rather than the machine's ID.
    #[must_use]
    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    /// Waits up to `wait_s` for the hub's feed to show a change, then verifies the shared
    /// candidate if it is due. Returns the candidate and how it ended, or `None` when there was
    /// nothing to verify. The daemon splits this in two: its tip loop waits on the feed and
    /// selects the target, and a verb process runs
    /// [`TipVerifier::round_at`] (see `round`).
    pub fn round(&mut self, ctx: &Context, wait_s: u32) -> Result<Option<(Commit, TipOutcome)>> {
        let batch = ctx.hub.feed(self.since, wait_s)?;
        self.since = self.since.max(batch.seq);
        let Some(tip) = self.next_target(ctx)? else {
            return Ok(None);
        };
        let outcome = self.round_at(ctx, &tip, self.since)?;
        Ok(Some((tip, outcome)))
    }

    /// The feed position already seen.
    pub fn since(&self) -> Seq {
        self.since
    }

    /// Whether `tip` needs a round: it was not settled, and is not resting after a round that
    /// ended waiting.
    pub fn due(&self, ctx: &Context, tip: &Commit) -> bool {
        let notice = self.fleet_notice();
        if notice
            .as_ref()
            .is_some_and(|notice| notice.state == ignite_core::fleet::MachineState::Off)
        {
            return false;
        }
        if self.fleet_version != notice.map(|notice| notice.version) {
            return true;
        }
        if self.settled.as_ref() == Some(tip) {
            return false;
        }
        !matches!(&self.resting, Some((resting, until)) if resting == tip && ctx.clock.now() < *until)
    }

    /// One round: verifies `tip`, remembers whether it settled or rests, and saves the memory
    /// with the feed position `since` the round was started at.
    pub fn round_at(&mut self, ctx: &Context, tip: &Commit, since: Seq) -> Result<TipOutcome> {
        self.since = self.since.max(since);
        let now = ctx.clock.now();
        let fleet_version = self.fleet_notice().map(|notice| notice.version);
        let outcome = self.verify(ctx, tip)?;
        self.fleet_version = fleet_version;
        if self.settled.as_ref() == Some(tip) {
            self.settled = None;
        }
        self.resting = None;
        match &outcome {
            TipOutcome::Verified
            | TipOutcome::VerifiedWithDeferrals { .. }
            | TipOutcome::Failed { .. } => {
                self.settled = Some(tip.clone());
            }
            TipOutcome::Waiting { checks, unrunnable } => {
                eprintln!(
                    "ignite: tip {tip} waits for {} checks, first {}; trying again in {} minutes",
                    checks.len(),
                    checks.first().map_or("none", CheckName::as_str),
                    WAITING_RETRY_S / 60.0
                );
                for line in unrunnable {
                    eprintln!("ignite: tip {tip} can never verify: {line}");
                }
                self.resting = Some((tip.clone(), now.plus_seconds(WAITING_RETRY_S)));
            }
            TipOutcome::Superseded { .. } => {}
        }
        self.save(ctx);
        Ok(outcome)
    }
}

fn verdict(check: &TipCheck, outcome: Outcome, output: Option<String>) -> TipRun {
    TipRun {
        check: check.check.clone(),
        measurement_hash: check.machine_relative.then(|| check.key.hash.clone()),
        outcome,
        output,
        reason: None,
        build: None,
    }
}

/// Files each run's outcome under the key of the runnable check it names.
/// The outputs of these runs are files this machine wrote, so `own` remembers them for [`tail`].
fn settle_runs(
    checks: &[TipCheck],
    runs: Vec<TipRun>,
    verdicts: &mut HashMap<ResultKey, TipRun>,
    own: &mut HashSet<String>,
) {
    for run in runs {
        own.extend(run.output.clone());
        if let Some(check) = checks.iter().find(|c| c.runnable && c.check == run.check)
            && (!check.machine_relative
                || run.outcome == Outcome::Incomplete
                || run.measurement_hash.as_ref() == Some(&check.key.hash))
        {
            verdicts.insert(check.key.clone(), run);
        }
    }
}

/// Whether any changed path is one of `inputs` or lies inside one of them.
fn touches(changed: &[String], inputs: &[String]) -> bool {
    changed.iter().any(|path| {
        inputs.iter().any(|input| {
            input.is_empty()
                || input == "."
                || path == input
                || path
                    .strip_prefix(input.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
        })
    })
}

fn names_of(checks: &[&TipCheck]) -> Vec<CheckName> {
    let mut names: Vec<CheckName> = checks.iter().map(|c| c.check.clone()).collect();
    names.sort();
    names.dedup();
    names
}

/// The last lines of a failure's output: the file's, when `output` is a file that a run on this
/// machine wrote (one of `own`), and otherwise the reference itself.
///
/// A cached result carries the reference that the machine that ran it stored, which on another
/// machine can be any path, so it is printed and never opened.
fn tail(output: &str, own: &HashSet<String>) -> String {
    let text = own
        .contains(output)
        .then(|| fs::read_to_string(output).ok())
        .flatten();
    // An empty file says nothing, so the reference is printed in its place.
    let Some(text) = text.filter(|text| !text.trim().is_empty()) else {
        return output.to_owned();
    };
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(OUTPUT_LINES)..].join("\n")
}
