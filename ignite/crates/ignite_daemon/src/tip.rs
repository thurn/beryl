//! Tip verification: once per `staging` tip, run the regression checks the gate left out, move
//! `master` forward when they all pass, and on a failure find the landing that caused it and
//! report a regression (see "Tip verification" and "Finding the culprit" in `docs/ignite.md`).
//!
//! - A round starts when the hub's change feed shows something new. It verifies the newest
//!   `staging` commit; tips in between are never verified one by one.
//! - The set: every regression check that the hub has not switched off, among the families this
//!   machine runs, with no cached result for the tip. A check whose inputs did not change since
//!   the last verified commit has the same input hash, so the result cache answers it.
//! - The set is claimed from the hub in batches of one family's checks, so two machines never
//!   run the same batch. Checks only this machine's platform can run go first.
//! - `master` moves to the tip once every check in its set has a pass on each platform it
//!   needs. A failure or an `incomplete` check holds it. A failure cached by another machine is
//!   run once here before it holds `master`.
//! - A failure's candidates are the landings since the last verified commit that changed one
//!   of the check's inputs. One candidate is the culprit with nothing run; several are searched
//!   by running that check between them, at most [`MAX_RUNS`] times. A landing is one
//!   ticket's commits, as the hub recorded them when the ticket landed, so a report names the
//!   ticket whose landing caused the failure.
//! - When the tips since the last verified commit have waited [`MACHINE_WAIT_S`] for checks
//!   only another platform's machines can run, the hub is told once per tip and platform, and
//!   files a P1 ticket naming those machines. `master` keeps waiting.

mod work;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::sync::Arc;

use ignite_core::config::Os;
use ignite_core::ids::{CheckName, Commit, Seq, TicketId};
use ignite_core::protocol::hub::{
    Record, RegressionReport, ResultKey, TipClaim, TipState, TipVerified, TipWaiting,
};
use ignite_core::protocol::{Class, Outcome};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Error, Push, Result};
use ignite_git::changed::CommitPaths;

use crate::server::Context;

pub use work::CheckTipWork;

/// How many of one family's checks one claim covers.
pub const BATCH: usize = 50;

/// The most runs one culprit search may make before it reports a range.
pub const MAX_RUNS: u32 = 10;

/// How long tips may wait for checks only another platform's machines can run before the hub
/// files a ticket naming those machines.
pub const MACHINE_WAIT_S: f64 = 3_600.0;

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
    fn checks(&self, ctx: &Context, commit: &Commit) -> Result<Vec<TipCheck>>;
    /// Runs the named checks at `commit` in a landing tree, in the `tip` class, storing their
    /// results. `rerun` ignores cached results.
    fn run(&self, ctx: &Context, commit: &Commit, checks: &[CheckName], rerun: bool)
    -> Vec<TipRun>;
    /// The landings after `from` up to and including `to`, oldest first.
    fn landings(&self, ctx: &Context, from: &Commit, to: &Commit) -> Result<Vec<TipLanding>>;
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
}

/// Verifies tips one round at a time. The daemon keeps one and calls [`TipVerifier::round`] in
/// a loop on its own thread.
pub struct TipVerifier {
    work: Arc<dyn TipWork>,
    /// The Ignite commit that records name.
    ignite: String,
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
}

impl TipVerifier {
    pub fn new(work: Arc<dyn TipWork>, ignite: impl Into<String>) -> Self {
        Self {
            work,
            ignite: ignite.into(),
            since: Seq(0),
            settled: None,
            reported: HashSet::new(),
            waiting_since: BTreeMap::new(),
            flagged: HashSet::new(),
        }
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
        let outcome = self.verify(ctx, &tip)?;
        if matches!(outcome, TipOutcome::Verified | TipOutcome::Failed { .. }) {
            self.settled = Some(tip.clone());
        }
        Ok(Some((tip, outcome)))
    }

    /// Verifies one tip: runs what its set still needs, then moves `master`, reports failures
    /// or waits.
    pub fn verify(&mut self, ctx: &Context, tip: &Commit) -> Result<TipOutcome> {
        let repository = &ctx.config.repository;
        let last = ctx.git.tip(&repository.master)?;
        if last == *tip {
            return Ok(TipOutcome::Verified);
        }
        let checks = self.work.checks(ctx, tip)?;
        let keys: Vec<ResultKey> = checks.iter().map(|c| c.key.clone()).collect();
        let cached = ctx.hub.lookup(&keys)?;
        let mut verdicts: HashMap<ResultKey, TipRun> = HashMap::new();
        let mut to_run: Vec<&TipCheck> = Vec::new();
        let mut recheck: Vec<&TipCheck> = Vec::new();
        for (check, hit) in checks.iter().zip(cached) {
            match hit.filter(|_| check.reuse) {
                Some(hit)
                    if hit.outcome == Outcome::Fail
                        && check.runnable
                        && hit.machine != ctx.machine =>
                {
                    recheck.push(check);
                }
                Some(hit) => {
                    verdicts.insert(check.key.clone(), verdict(check, hit.outcome, hit.output));
                }
                None if check.runnable && check.reuse => to_run.push(check),
                None if check.runnable => recheck.push(check),
                None => {}
            }
        }
        self.record_cached(ctx, tip, &checks, &to_run, &recheck)?;

        for (index, (family, batch, names)) in batches(&to_run).into_iter().enumerate() {
            if index > 0 {
                let newest = ctx.git.tip(&repository.staging)?;
                if newest != *tip {
                    return Ok(TipOutcome::Superseded { by: newest });
                }
            }
            let claim = TipClaim {
                commit: tip.clone(),
                family,
                batch,
                machine: ctx.machine.clone(),
            };
            if !ctx.hub.tip_claim(&claim)?.granted {
                continue;
            }
            let runs = self.work.run(ctx, tip, &names, false);
            settle_runs(&checks, runs, &mut verdicts);
        }
        if !recheck.is_empty() {
            let names: Vec<CheckName> = recheck.iter().map(|c| c.check.clone()).collect();
            let runs = self.work.run(ctx, tip, &names, true);
            settle_runs(&checks, runs, &mut verdicts);
        }
        Self::fill_from_cache(ctx, &checks, &mut verdicts)?;

        let mut failed: Vec<&TipCheck> = Vec::new();
        let mut waiting: Vec<CheckName> = Vec::new();
        let mut elsewhere: BTreeMap<Os, Vec<CheckName>> = BTreeMap::new();
        for check in &checks {
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
        if !failed.is_empty() {
            for check in &failed {
                let output = verdicts.get(&check.key).and_then(|v| v.output.clone());
                self.blame(ctx, &last, tip, check, output.as_deref())?;
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
        self.move_master(ctx, tip)
    }

    /// Moves `master` to a tip whose every check passed and records it as verified.
    fn move_master(&mut self, ctx: &Context, tip: &Commit) -> Result<TipOutcome> {
        let repository = &ctx.config.repository;
        match ctx.git.push(&ctx.repo, tip, &repository.master)? {
            Push::Pushed { .. } => {}
            Push::Rejected { reason } => {
                return Err(Error::Conflict(format!(
                    "{} did not move to {tip}: {reason}",
                    repository.master
                )));
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

    /// The landing between `good` and `bad` that made `check` fail, or the range of landings a
    /// search that ran out of runs ended with. `None` when no commit lies between them.
    pub fn find_culprit(
        &self,
        ctx: &Context,
        check: &TipCheck,
        good: &Commit,
        bad: &Commit,
    ) -> Result<Option<Culprit>> {
        let landings = self.work.landings(ctx, good, bad)?;
        if landings.is_empty() {
            return Ok(None);
        }
        let touching: Vec<usize> = (0..landings.len())
            .filter(|&i| touches(&landings[i].paths, &check.inputs))
            .collect();
        // A failure that no landing explains (a changed toolchain, say) blames every landing.
        let candidates: Vec<usize> = if touching.is_empty() {
            (0..landings.len()).collect()
        } else {
            touching
        };
        // Only candidates change what the check reads, so its result can change only at a
        // candidate's last commit. Position `i` is "after the i-th candidate": 0 passes, as
        // `good` did, and `candidates.len()` fails, as `bad` did.
        let after = |i: usize| -> Option<Commit> { landings[candidates[i - 1]].last().cloned() };
        let (mut low, mut high, mut runs) = (0, candidates.len(), 0);
        while high - low > 1 && runs < MAX_RUNS {
            let middle = usize::midpoint(low, high);
            let Some(at) = after(middle) else { break };
            let outcome = self
                .work
                .run(ctx, &at, std::slice::from_ref(&check.check), false)
                .into_iter()
                .find(|r| r.check == check.check)
                .map(|r| r.outcome);
            runs += 1;
            match outcome {
                Some(Outcome::Pass | Outcome::Flaky) => low = middle,
                Some(Outcome::Fail) => high = middle,
                // A run that says nothing cannot narrow the range.
                Some(Outcome::Incomplete) | None => break,
            }
        }
        let first_suspect = candidates[low];
        let last_good = match first_suspect.checked_sub(1) {
            Some(previous) => landings[previous].last().cloned(),
            None => Some(good.clone()),
        };
        let (Some(first_bad), Some(last_good)) = (after(high), last_good) else {
            return Ok(None);
        };
        let suspects = candidates[low..high]
            .iter()
            .map(|&i| landings[i].clone())
            .collect();
        Ok(Some(Culprit {
            suspects,
            first_bad,
            last_good,
            runs,
        }))
    }

    /// Finds the culprit of a check that failed at `tip` and reports the regression, unless a
    /// newer tip changed the check's inputs (its next round runs the check there first) or this
    /// failure was reported already.
    ///
    /// A check only another platform can run is left to that platform's machines: a search here
    /// would run this platform's version of the check, which passes at every commit, and blame
    /// the newest candidate whatever the real culprit.
    fn blame(
        &mut self,
        ctx: &Context,
        last: &Commit,
        tip: &Commit,
        check: &TipCheck,
        output: Option<&str>,
    ) -> Result<()> {
        if !check.runnable || self.reported.contains(&check.key) {
            return Ok(());
        }
        let newest = ctx.git.tip(&ctx.config.repository.staging)?;
        if newest != *tip && touches(&ctx.git.changed(tip, &newest)?, &check.inputs) {
            return Ok(());
        }
        let Some(culprit) = self.find_culprit(ctx, check, last, tip)? else {
            return Ok(());
        };
        let tickets: Vec<TicketId> = culprit
            .suspects
            .iter()
            .filter_map(|l| l.ticket.clone())
            .collect();
        let (caused_by, suspects) = if culprit.suspects.len() == 1 {
            (tickets.into_iter().next(), Vec::new())
        } else {
            (None, tickets)
        };
        ctx.hub.regression(&RegressionReport {
            check: check.check.clone(),
            first_bad: culprit.first_bad,
            last_good: culprit.last_good,
            caused_by,
            output: output.map(tail).unwrap_or_default(),
            suspects,
            platform: check.key.platform,
        })?;
        self.reported.insert(check.key.clone());
        Ok(())
    }

    /// Looks up the checks still without a verdict, which other machines may have passed by
    /// now. A failure from another machine that this one can run is left for the next round to
    /// run here.
    fn fill_from_cache(
        ctx: &Context,
        checks: &[TipCheck],
        verdicts: &mut HashMap<ResultKey, TipRun>,
    ) -> Result<()> {
        let missing: Vec<&TipCheck> = checks
            .iter()
            .filter(|c| c.reuse && !verdicts.contains_key(&c.key))
            .collect();
        if missing.is_empty() {
            return Ok(());
        }
        let keys: Vec<ResultKey> = missing.iter().map(|c| c.key.clone()).collect();
        for (check, hit) in missing.into_iter().zip(ctx.hub.lookup(&keys)?) {
            let Some(hit) = hit else { continue };
            let foreign_failure =
                hit.outcome == Outcome::Fail && check.runnable && hit.machine != ctx.machine;
            if !foreign_failure {
                verdicts.insert(check.key.clone(), verdict(check, hit.outcome, hit.output));
            }
        }
        Ok(())
    }

    /// Sends one record with the verb `cached` and no cost for each family whose checks were
    /// all answered by the result cache, so the hub can count cache hits.
    fn record_cached(
        &self,
        ctx: &Context,
        tip: &Commit,
        checks: &[TipCheck],
        to_run: &[&TipCheck],
        recheck: &[&TipCheck],
    ) -> Result<()> {
        let families: BTreeSet<&str> = checks
            .iter()
            .filter(|c| c.runnable)
            .map(|c| c.family.as_str())
            .collect();
        let records: Vec<Record> = families
            .into_iter()
            .filter(|family| !to_run.iter().chain(recheck).any(|c| c.family == *family))
            .map(|family| Record {
                machine: ctx.machine.clone(),
                ticket: None,
                session: None,
                tree: None,
                verb: "cached".to_owned(),
                class: Class::Tip,
                family: Some(family.to_owned()),
                queued_s: 0.0,
                wall_s: 0.0,
                cpu_s: 0.0,
                instructions: None,
                peak_mb: 0,
                outcome: Some(Outcome::Pass),
                commit: Some(tip.clone()),
                ignite: self.ignite.clone(),
                at: ctx.clock.now(),
                compile_units: Vec::new(),
            })
            .collect();
        if records.is_empty() {
            return Ok(());
        }
        ctx.hub.records(&records)
    }
}

/// Groups `commits`, oldest first, into landings by the ticket the hub says landed each one.
/// A run of commits no landing recorded, such as a push by hand, is a landing with no ticket.
pub fn ticketed_landings(ctx: &Context, commits: Vec<CommitPaths>) -> Result<Vec<TipLanding>> {
    let shas: Vec<Commit> = commits.iter().map(|c| c.commit.clone()).collect();
    let owners: HashMap<Commit, TicketId> = ctx
        .hub
        .landed_commits(&shas)?
        .into_iter()
        .map(|landed| (landed.commit, landed.ticket))
        .collect();
    let owner = |i: usize| owners.get(&shas[i]);
    let ends: Vec<Commit> = (0..shas.len())
        .filter(|&i| i + 1 == shas.len() || owner(i) != owner(i + 1))
        .map(|i| shas[i].clone())
        .collect();
    Ok(ignite_git::changed::group(commits, &ends)
        .into_iter()
        .map(|landing| TipLanding {
            ticket: landing.commits.last().and_then(|c| owners.get(c)).cloned(),
            commits: landing.commits,
            paths: landing.paths,
        })
        .collect())
}

fn verdict(check: &TipCheck, outcome: Outcome, output: Option<String>) -> TipRun {
    TipRun {
        check: check.check.clone(),
        outcome,
        output,
    }
}

/// Files each run's outcome under the key of the runnable check it names.
fn settle_runs(checks: &[TipCheck], runs: Vec<TipRun>, verdicts: &mut HashMap<ResultKey, TipRun>) {
    for run in runs {
        if let Some(check) = checks.iter().find(|c| c.runnable && c.check == run.check) {
            verdicts.insert(check.key.clone(), run);
        }
    }
}

/// The checks to run, in claimable batches: one family's checks sorted by name and cut every
/// [`BATCH`], so every machine cuts the same batches. Families with checks only this machine's
/// platform can run come first.
fn batches(to_run: &[&TipCheck]) -> Vec<(String, u32, Vec<CheckName>)> {
    let mut families: BTreeMap<(bool, &str), Vec<CheckName>> = BTreeMap::new();
    for check in to_run {
        let portable = check.key.platform.is_none();
        families
            .entry((portable, check.family.as_str()))
            .or_default()
            .push(check.check.clone());
    }
    let mut batches = Vec::new();
    for ((_, family), mut names) in families {
        names.sort();
        names.dedup();
        for (index, chunk) in names.chunks(BATCH).enumerate() {
            let batch = u32::try_from(index).unwrap_or(u32::MAX);
            batches.push((family.to_owned(), batch, chunk.to_vec()));
        }
    }
    batches
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

/// The last lines of a failure's output: the file's, when `output` names a readable file, and
/// otherwise the reference itself.
fn tail(output: &str) -> String {
    let Ok(text) = fs::read_to_string(output) else {
        return output.to_owned();
    };
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(OUTPUT_LINES)..].join("\n")
}
