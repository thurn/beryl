//! Tip verification: once per `staging` tip, run the regression checks the gate left out, move
//! `master` forward when they all pass, and on a failure find the landing that caused it and
//! report a regression (see "Tip verification" and "Finding the culprit" in `docs/ignite.md`).
//!
//! - A round starts when the hub's change feed shows something new. It verifies the newest
//!   `staging` commit; tips in between are never verified one by one.
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
//! - A round that has run a batch is finished even when a newer tip arrives, so a shared input
//!   that every landing changes cannot keep any tip from being verified.
//! - A failure's candidates are the landings since the last verified commit that changed one
//!   of the check's inputs. One candidate is the culprit with nothing run; several are searched
//!   by running that check between them, at most [`MAX_RUNS`] times. A landing is one
//!   ticket's commits, as the hub recorded them when the ticket landed, so a report names the
//!   ticket whose landing caused the failure.
//! - When the tips since the last verified commit have waited [`MACHINE_WAIT_S`] for checks
//!   only another platform's machines can run, the hub is told once per tip and platform, and
//!   files a P1 ticket naming those machines. `master` keeps waiting.

mod cache;
mod covered;
mod culprit;
mod families;
mod meter;
mod report;
mod split;
mod work;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::sync::Arc;

use ignite_core::config::Os;
use ignite_core::ids::{CheckName, Commit, Seq, TicketId};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{IncompleteReason, ResultKey, TipState, TipVerified, TipWaiting};
use ignite_core::time::Timestamp;
use ignite_core::tipround::CulpritSearch;
use ignite_core::traits::{Push, Result};

use crate::checks::Scoring;
use crate::server::Context;

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
}

/// One check's outcome from a run at one commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TipRun {
    pub check: CheckName,
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
    /// The platforms whose machines can run a family this machine does not.
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

/// How one tip's verification ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TipOutcome {
    /// Every check passed and `master` is at the tip.
    Verified,
    /// These checks have no result yet: another machine holds their batch, only another
    /// platform can run them, or their run was incomplete. `master` waits.
    Waiting { checks: Vec<CheckName> },
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
    /// Failures already reported, so a cached failure that stands for later tips is reported
    /// once.
    reported: HashSet<ResultKey>,
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
}

impl TipVerifier {
    pub fn new(work: Arc<dyn TipWork>, ignite: impl Into<String>) -> Self {
        Self {
            work,
            ignite: ignite.into(),
            host: String::new(),
            since: Seq(0),
            settled: None,
            reported: HashSet::new(),
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
        }
    }

    /// Names this machine's hostname in the reports of checks that stay incomplete, so the
    /// hub's tickets say `jarvis` rather than the machine's ID.
    #[must_use]
    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    /// Waits up to `wait_s` for the hub's feed to show a change, then verifies the newest
    /// `staging` commit unless it was already settled. Returns that tip and how it ended, or
    /// `None` when there was nothing to verify.
    pub fn round(&mut self, ctx: &Context, wait_s: u32) -> Result<Option<(Commit, TipOutcome)>> {
        let batch = ctx.hub.feed(self.since, wait_s)?;
        self.since = self.since.max(batch.seq);
        let tip = ctx.git.tip(&ctx.config.repository.staging)?;
        if self.settled.as_ref() == Some(&tip) {
            return Ok(None);
        }
        let now = ctx.clock.now();
        if let Some((resting, until)) = &self.resting
            && *resting == tip
            && now < *until
        {
            return Ok(None);
        }
        let outcome = self.verify(ctx, &tip)?;
        self.resting = None;
        match &outcome {
            TipOutcome::Verified | TipOutcome::Failed { .. } => self.settled = Some(tip.clone()),
            TipOutcome::Waiting { checks } => {
                eprintln!(
                    "ignite: tip {tip} waits for {} checks, first {}; trying again in {} minutes",
                    checks.len(),
                    checks.first().map_or("none", CheckName::as_str),
                    WAITING_RETRY_S / 60.0
                );
                self.resting = Some((tip.clone(), now.plus_seconds(WAITING_RETRY_S)));
            }
            TipOutcome::Superseded { .. } => {}
        }
        Ok(Some((tip, outcome)))
    }

    /// Verifies one tip: runs what its set still needs, then moves `master`, reports failures
    /// or waits. A round that ran sends the hub a record of where its time went.
    pub fn verify(&mut self, ctx: &Context, tip: &Commit) -> Result<TipOutcome> {
        crate::steps::step(format!("verifying tip {tip}"));
        let last = ctx.git.tip(&ctx.config.repository.master)?;
        if last == *tip {
            // `master` already holds the tip: another machine verified it, or someone pushed it
            // by hand. The hub records it, so that it and older tips stop showing as waiting.
            ctx.hub.tip_verified(&TipVerified {
                commit: tip.clone(),
                machine: ctx.machine.clone(),
                state: TipState::Verified,
            })?;
            return Ok(TipOutcome::Verified);
        }
        let (meter, metered) = meter::Meter::start(ctx);
        let mut tally = meter::Tally::default();
        let outcome = self.verify_round(&metered, &last, tip, &mut tally);
        meter::send(ctx, &meter.finish(ctx, tip, &tally));
        outcome
    }

    /// [`TipVerifier::verify`] once `master` is known to be behind the tip at `last`, counting
    /// in `tally` the checks run and answered by the cache.
    #[allow(clippy::too_many_lines)]
    fn verify_round(
        &mut self,
        ctx: &Context,
        last: &Commit,
        tip: &Commit,
        tally: &mut meter::Tally,
    ) -> Result<TipOutcome> {
        self.own_outputs.clear();
        let TipSet {
            checks,
            unlisted,
            off,
        } = self.work.checks(ctx, tip)?;
        // Batches are cut over the listing alone, before checks switched off as flaky or
        // covered by another family are left out, so that every machine cuts them alike.
        self.listed = checks
            .iter()
            .chain(&off)
            .filter(|c| c.runnable)
            .map(|c| Listed {
                family: c.family.clone(),
                check: c.check.clone(),
                portable: c.key.platform.is_none(),
            })
            .collect();
        let checks = covered::drop_covered(checks, &self.work.covered_by());
        let now = ctx.clock.now();
        let keys: Vec<ResultKey> = checks.iter().map(|c| c.key.clone()).collect();
        let cached = ctx.hub.lookup(&keys)?;
        let mut verdicts: HashMap<ResultKey, TipRun> = HashMap::new();
        let mut to_run: Vec<&TipCheck> = Vec::new();
        let mut recheck: Vec<&TipCheck> = Vec::new();
        for (check, hit) in checks.iter().zip(cached) {
            let held = self
                .held_off
                .get(&check.key)
                .is_some_and(|until| now < *until);
            match hit.filter(|_| check.reuse) {
                // A failure is run again once here before it holds `master`, whoever stored it:
                // another machine's may be a fault of that machine, this one's a fault of one
                // run. The rerun's result is stored over it, and stands afterwards.
                Some(hit)
                    if hit.outcome == Outcome::Fail
                        && check.runnable
                        && !held
                        && (hit.machine != ctx.machine
                            || self.rechecked.insert(check.key.clone())) =>
                {
                    recheck.push(check);
                }
                Some(hit) => {
                    verdicts.insert(check.key.clone(), verdict(check, hit.outcome, hit.output));
                }
                None if held => {}
                None if check.runnable && check.reuse => to_run.push(check),
                None if check.runnable => recheck.push(check),
                None => {}
            }
        }
        self.record_cached(ctx, tip, &checks, &to_run, &recheck);
        tally.cached = u32::try_from(verdicts.len()).unwrap_or(u32::MAX);

        // The tip budget: the best-scored checks run before `master` moves, the rest after.
        let split = self.split(ctx, last, tip, &to_run, &recheck);
        tally.budget.clone_from(&split.budget);
        let (superseded, runs) = self.run_families(ctx, tip, &split.within)?;
        if let Some(by) = superseded {
            return Ok(TipOutcome::Superseded { by });
        }
        tally.ran = u32::try_from(runs.len()).unwrap_or(u32::MAX);
        settle_runs(&checks, runs, &mut verdicts, &mut self.own_outputs);
        if !recheck.is_empty() {
            let names: Vec<CheckName> = recheck.iter().map(|c| c.check.clone()).collect();
            let runs = self.work.run(ctx, tip, &names, true);
            tally.ran = tally
                .ran
                .saturating_add(u32::try_from(runs.len()).unwrap_or(u32::MAX));
            settle_runs(&checks, runs, &mut verdicts, &mut self.own_outputs);
        }
        Self::fill_from_cache(ctx, &checks, &mut verdicts)?;
        self.note_incomplete(ctx, tip, &checks, &verdicts);
        self.report_unlisted(ctx, tip, &unlisted);

        let mut failed: Vec<&TipCheck> = Vec::new();
        let mut waiting: Vec<CheckName> = Vec::new();
        let mut elsewhere: BTreeMap<Os, Vec<CheckName>> = BTreeMap::new();
        for check in checks.iter().filter(|c| !split.is_after(c)) {
            match verdicts.get(&check.key).map(|v| v.outcome) {
                Some(Outcome::Pass | Outcome::Flaky) => {}
                Some(Outcome::Fail) => failed.push(check),
                Some(Outcome::Incomplete) | None => {
                    waiting.push(check.check.clone());
                    if let (false, Some(os)) = (check.runnable, check.key.platform) {
                        elsewhere.entry(os).or_default().push(check.check.clone());
                    }
                }
            }
        }
        for missing in &unlisted {
            waiting.push(missing.name.clone());
            for os in &missing.platforms {
                elsewhere.entry(*os).or_default().push(missing.name.clone());
            }
        }
        if !failed.is_empty() {
            for check in &failed {
                let output = verdicts.get(&check.key).and_then(|v| v.output.clone());
                // A failure to find the culprit or to reach the hub must not keep the tip from
                // being recorded as failed; a later tip that still fails reports it again.
                if let Err(error) = self.blame(ctx, last, tip, check, output.as_deref()) {
                    eprintln!(
                        "ignite: regression of {} not reported: {error}",
                        check.check
                    );
                }
            }
            ctx.hub.tip_verified(&TipVerified {
                commit: tip.clone(),
                machine: ctx.machine.clone(),
                state: TipState::Failed,
            })?;
            return Ok(TipOutcome::Failed {
                checks: names_of(&failed),
            });
        }
        if !waiting.is_empty() {
            self.flag_missing_machines(ctx, tip, elsewhere)?;
            waiting.sort();
            waiting.dedup();
            return Ok(TipOutcome::Waiting { checks: waiting });
        }
        let outcome = self.move_master(ctx, tip)?;
        if outcome == TipOutcome::Verified {
            self.run_after(ctx, last, tip, &checks, &split, &mut verdicts, tally)?;
        }
        Ok(outcome)
    }

    /// Moves `master` to a tip whose every check passed and records it as verified.
    fn move_master(&mut self, ctx: &Context, tip: &Commit) -> Result<TipOutcome> {
        let repository = &ctx.config.repository;
        match ctx.git.push(&ctx.repo, tip, &repository.master)? {
            Push::Pushed { .. } => {}
            Push::Rejected { reason } => {
                // Typically `master` holds a commit `staging` lacks, such as a push by hand.
                // Trying again gives the same answer, so the hub is told once.
                let name = CheckName::new("tip:move-master")
                    .unwrap_or_else(|_| unreachable!("a family and a word make a check name"));
                let output = format!("{} did not move to {tip}: {reason}", repository.master);
                self.report_stuck(ctx, tip, &name, None, &output);
                return Ok(TipOutcome::Waiting { checks: vec![name] });
            }
        }
        ctx.hub.tip_verified(&TipVerified {
            commit: tip.clone(),
            machine: ctx.machine.clone(),
            state: TipState::Verified,
        })?;
        self.waiting_since.clear();
        self.flagged.clear();
        Ok(TipOutcome::Verified)
    }

    /// Starts the wait for each platform `tip` waits on that no earlier tip was waiting on,
    /// forgets platforms it no longer waits on, and reports each platform waited on for
    /// [`MACHINE_WAIT_S`], once per tip.
    fn flag_missing_machines(
        &mut self,
        ctx: &Context,
        tip: &Commit,
        elsewhere: BTreeMap<Os, Vec<CheckName>>,
    ) -> Result<()> {
        let now = ctx.clock.now();
        self.waiting_since
            .retain(|os, _| elsewhere.contains_key(os));
        for (platform, mut checks) in elsewhere {
            let since = *self.waiting_since.entry(platform).or_insert(now);
            let waited_s = now.seconds_since(since);
            if waited_s < MACHINE_WAIT_S || self.flagged.contains(&(tip.clone(), platform)) {
                continue;
            }
            checks.sort();
            checks.dedup();
            ctx.hub.tip_waiting(&TipWaiting {
                commit: tip.clone(),
                machine: ctx.machine.clone(),
                platform,
                checks,
                waited_s,
            })?;
            self.flagged.insert((tip.clone(), platform));
        }
        Ok(())
    }
}

fn verdict(check: &TipCheck, outcome: Outcome, output: Option<String>) -> TipRun {
    TipRun {
        check: check.check.clone(),
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
        if let Some(check) = checks.iter().find(|c| c.runnable && c.check == run.check) {
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
    let Some(text) = text else {
        return output.to_owned();
    };
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(OUTPUT_LINES)..].join("\n")
}
