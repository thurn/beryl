//! Running a round's checks: each family's batches from one listing, several families at once.
//!
//! A family's batches are cut from every check of the family this platform lists at the tip,
//! not only those this machine still has to run, with the costs the hub fixed for the tip
//! ([`ignite_core::tipround`]). Checks the hub switched off as flaky and checks another family
//! covers are cut over too, so a check switched off partway through a round does not shift
//! the batches of a machine that listed the family after it. So every machine cuts the same
//! batches, and a batch number names the same checks on each, however many of them another
//! machine has run meanwhile. A batch is then run for the checks in it that still need a run
//! here, and one with none is not claimed.
//!
//! The batches are claimed from the hub one at a time, right before each runs, so two machines
//! never run the same batch and a claim never lapses while its batch waits. While a batch runs,
//! the round claims it again every [`RENEW_EVERY`], which renews the claim, so a batch that runs
//! longer than the hub's claim (15 minutes) is not taken by another machine meanwhile. The round
//! runs in a verb process of its own (see `round`), so its claims are renewed across a restart
//! of the daemon too. Families run on up to [`TipWork::parallel`] threads, each in its own
//! landing tree; every command still asks the scheduler for its cores in the `tip` class, so a
//! busy machine runs fewer at once.
//!
//! Before a family's thread leases a tree for it, the round asks the hub which of the family's
//! checks to run here still need a run: no other machine holds their batch's claim or has
//! stored their result (`Round::left_elsewhere`). Leasing a tree for a family lists it, and
//! listing builds its products. A slow machine used to build a family for many minutes and then
//! find that faster machines had claimed every batch meanwhile, so it ran nothing. A family with
//! none left is skipped and counted as left to other machines ([`Elsewhere`]), not as unrun; it
//! stores no verdict, so a later round still runs the checks if those claims lapse. A hub that
//! cannot answer leaves the family to be built as before, so a failed lookup never loses a check.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use ignite_core::config::Os;
use ignite_core::ids::{CheckName, Commit};
use ignite_core::protocol::hub::{
    ResultKey, TipCheckFailures, TipClaim, TipUnclaimed, TipUnclaimedBatch,
};
use ignite_core::tipround::{self, TipCosts, TipCostsRequest};
use ignite_core::traits::Result;

use super::{Listed, TipCheck, TipRun, TipVerifier, candidate};
use crate::checks::BatchCallbacks;
use crate::server::Context;

/// How often a running batch's claim is renewed: a third of the hub's claim
/// ([`ignite_store::hub::tips::CLAIM_SECONDS`]), so two renewals can fail before it lapses.
pub const RENEW_EVERY: Duration = Duration::from_secs(300);

/// One family's batches, in the order every machine cuts them.
type FamilyBatches = (String, Vec<(u32, Vec<CheckName>)>);

/// The checks a round skipped, by family, because when it came to their family every one of
/// them was claimed or answered by another machine (see the module documentation).
pub(super) type Elsewhere = BTreeMap<String, Vec<CheckName>>;

/// What [`TipVerifier::run_families`] returns: the newer tip that ended the round, if one did,
/// the runs, and the checks left to other machines.
pub(super) type FamilyRuns = (Option<Commit>, Vec<TipRun>, Elsewhere);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What the verifier knows about a family's checks when it runs them.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RunPhase {
    Build,
    BeforeMaster,
    AfterMaster,
}

/// What the threads of one round share.
struct Round<'a> {
    verifier: &'a TipVerifier,
    ctx: &'a Context,
    tip: &'a Commit,
    /// Whether `master` is already at `tip`, so that the round runs only checks past the tip
    /// budget. Such a round yields to a newer `staging` tip before any family or batch, even
    /// with work under way: the next round is what moves `master` now, and the checks left
    /// over are in the next tip's set.
    after_master: bool,
    /// Claim names for checks scoped to an OS or to an individual verifier.
    claim_scopes: BTreeMap<&'a str, (Os, bool)>,
    /// The result key of each check to run, which the hub looks up for a stored result.
    keys: HashMap<&'a CheckName, &'a ResultKey>,
    queue: Mutex<VecDeque<FamilyBatches>>,
    /// The commit that ended the round, or the error reading `staging` after promotion.
    ended: Mutex<Option<Result<Commit>>>,
    runs: Mutex<Vec<TipRun>>,
    elsewhere: Mutex<Elsewhere>,
    /// The batch each family is running under a claim, which the renewer claims again.
    held: Mutex<BTreeMap<String, u32>>,
}

impl Round<'_> {
    /// The next family to run, or `None` when none is left or the round should stop.
    fn next(&self) -> Option<FamilyBatches> {
        let next = lock(&self.queue).pop_front()?;
        (!self.should_stop()).then_some(next)
    }

    /// Before promotion, landings can keep advancing `staging` while both verifiers work
    /// on the same candidate. After promotion, a newer `staging` tip can use these trees.
    fn should_stop(&self) -> bool {
        if crate::steps::stop_requested()
            || self.verifier.machine_is_off()
            || lock(&self.ended).is_some()
        {
            return true;
        }
        let ended = if self.after_master {
            match self.ctx.git.tip(&self.ctx.config.repository.staging) {
                Ok(newest) if newest == *self.tip => None,
                other => Some(other),
            }
        } else {
            candidate::replacement(self.ctx, self.tip).map(Ok)
        };
        let mut state = lock(&self.ended);
        if state.is_none() {
            *state = ended;
        }
        state.is_some()
    }

    /// Claims one batch from the hub. A claim the hub cannot answer is not granted, so the
    /// batch is left for a later round.
    fn claim(&self, family: &str, batch: u32) -> bool {
        // A family asks for its next batch once the last one has finished.
        lock(&self.held).remove(family);
        if self.should_stop() {
            return false;
        }
        let granted = self.ask(family, batch);
        if granted {
            lock(&self.held).insert(family.to_owned(), batch);
        }
        granted
    }

    /// The name the hub keeps `family`'s claims under: scoped to an OS for checks keyed to one,
    /// and to this machine as well for checks whose results belong to the verifier.
    fn claim_name(&self, family: &str) -> String {
        match self.claim_scopes.get(family) {
            Some((os, true)) => format!("{family}@{os}@{}", self.ctx.machine),
            Some((os, false)) => format!("{family}@{os}"),
            None => family.to_owned(),
        }
    }

    /// Whether every check of `batches` is claimed or answered by another machine, so that this
    /// machine has nothing of `family` to run and need not build or list it. Records the checks
    /// in `elsewhere` when so. A hub that cannot answer leaves the family to run.
    fn left_elsewhere(&self, family: &str, batches: &[(u32, Vec<CheckName>)]) -> bool {
        let ask = TipUnclaimed {
            commit: self.tip.clone(),
            family: self.claim_name(family),
            machine: self.ctx.machine.clone(),
            batches: batches
                .iter()
                .map(|(batch, names)| TipUnclaimedBatch {
                    batch: *batch,
                    checks: names
                        .iter()
                        .filter_map(|name| self.keys.get(name).map(|key| (*key).clone()))
                        .collect(),
                })
                .collect(),
        };
        match self.ctx.hub.tip_unclaimed(&ask) {
            Ok(reply) if reply.checks.is_empty() => {
                let names: Vec<CheckName> = batches.iter().flat_map(|(_, n)| n.clone()).collect();
                eprintln!(
                    "ignite: tip verification: {family} not built: other machines hold or have \
                     answered all {} of its checks to run",
                    names.len()
                );
                lock(&self.elsewhere)
                    .entry(family.to_owned())
                    .or_default()
                    .extend(names);
                true
            }
            Ok(_) => false,
            Err(error) => {
                eprintln!(
                    "ignite: tip verification: building {family} without knowing whether other \
                     machines hold its checks: {error}"
                );
                false
            }
        }
    }

    /// Asks the hub for `batch` of `family`. The hub grants a batch this machine holds again,
    /// renewing its claim.
    fn ask(&self, family: &str, batch: u32) -> bool {
        let claim = TipClaim {
            commit: self.tip.clone(),
            family: self.claim_name(family),
            batch,
            machine: self.ctx.machine.clone(),
        };
        match self.ctx.hub.tip_claim(&claim) {
            Ok(reply) => reply.granted,
            Err(error) => {
                eprintln!(
                    "ignite: tip verification: batch {batch} of {family} not claimed: {error}"
                );
                false
            }
        }
    }

    /// Claims every running batch again each `every`, until `done`.
    fn renew_until(&self, done: &AtomicBool, every: Duration) {
        let nap = every.min(Duration::from_millis(100));
        let mut last = Instant::now();
        while !done.load(Ordering::SeqCst) {
            thread::sleep(nap);
            if last.elapsed() < every {
                continue;
            }
            last = Instant::now();
            let held: Vec<(String, u32)> = lock(&self.held)
                .iter()
                .map(|(family, batch)| (family.clone(), *batch))
                .collect();
            for (family, batch) in held {
                if !self.ask(&family, batch) {
                    eprintln!(
                        "ignite: tip verification: the claim on batch {batch} of {family} was \
                         not renewed; another machine may run it too"
                    );
                }
            }
        }
    }
}

impl TipVerifier {
    /// Runs `to_run`, some of the tip's checks, at `tip` and returns the runs, in batches cut
    /// over the round's listing. A replacement selected by the hub ends the round at the
    /// next family or batch.
    /// After `master` moves, a newer tip ends the round at the next family or batch
    /// whatever is under way. A family whose checks other machines all hold or have answered is
    /// skipped and returned in [`Elsewhere`].
    pub(super) fn run_families(
        &self,
        ctx: &Context,
        tip: &Commit,
        to_run: &[&TipCheck],
        phase: RunPhase,
    ) -> Result<FamilyRuns> {
        let after_master = phase == RunPhase::AfterMaster;
        let costs = |family: &str| family_costs(ctx, tip, family);
        let round = Round {
            verifier: self,
            ctx,
            tip,
            after_master,
            claim_scopes: to_run
                .iter()
                .filter_map(|check| {
                    Some((
                        check.family.as_str(),
                        (check.key.platform?, check.machine_relative),
                    ))
                })
                .collect(),
            keys: to_run
                .iter()
                .map(|check| (&check.check, &check.key))
                .collect(),
            queue: Mutex::new(by_family(batches(&self.listed, to_run, &costs)).into()),
            ended: Mutex::new(None),
            runs: Mutex::new(Vec::new()),
            elsewhere: Mutex::new(BTreeMap::new()),
            held: Mutex::new(BTreeMap::new()),
        };
        // A build check may be covered by a family we have not listed yet. Only report a
        // failure early when the check is already known to be required for master.
        let possibly_covered = (phase == RunPhase::Build).then(|| self.work.covered_by());
        let required: HashSet<&ResultKey> = to_run
            .iter()
            .filter(|check| {
                !after_master
                    && !possibly_covered
                        .as_ref()
                        .is_some_and(|families| families.contains_key(&check.family))
            })
            .map(|check| &check.key)
            .collect();
        let failed = |keys: &[ResultKey]| {
            let checks = keys
                .iter()
                .filter(|key| required.contains(*key))
                .cloned()
                .collect();
            observe_failures(ctx, tip, checks);
        };
        // Each family's thread works for the round's activity, so its builds run in the `tip`
        // class and `ignite status` shows which family it is on.
        let activity = crate::steps::current();
        let done = AtomicBool::new(false);
        thread::scope(|scope| {
            let workers: Vec<_> = (0..self.work.parallel().max(1))
                .map(|_| {
                    scope.spawn(|| {
                        let _entered = activity.as_ref().map(crate::steps::Activity::enter);
                        while let Some((family, batches)) = round.next() {
                            if round.left_elsewhere(&family, &batches) {
                                continue;
                            }
                            crate::steps::step(format!("verifying family {family}"));
                            let claim = |batch: u32| round.claim(&family, batch);
                            let callbacks = BatchCallbacks {
                                claim: &claim,
                                failed: &failed,
                            };
                            let (runs, _) = self
                                .work
                                .run_batches(ctx, tip, &family, &batches, &callbacks, !after_master);
                            lock(&round.held).remove(&family);
                            lock(&round.runs).extend(runs);
                        }
                    })
                })
                .collect();
            scope.spawn(|| round.renew_until(&done, self.renew_every));
            let mut panicked = None;
            for worker in workers {
                if let Err(panic) = worker.join() {
                    panicked.get_or_insert(panic);
                }
            }
            done.store(true, Ordering::SeqCst);
            if let Some(panic) = panicked {
                std::panic::resume_unwind(panic);
            }
        });
        super::round::require_running()?;
        self.require_machine_on()?;
        let superseded = round
            .ended
            .into_inner()
            .unwrap_or_else(PoisonError::into_inner)
            .transpose()?;
        let runs = round
            .runs
            .into_inner()
            .unwrap_or_else(PoisonError::into_inner);
        let elsewhere = round
            .elsewhere
            .into_inner()
            .unwrap_or_else(PoisonError::into_inner);
        Ok((superseded, runs, elsewhere))
    }
}

/// Lets newer staging compete when a required check has an exact failed result.
pub(super) fn observe_failures(ctx: &Context, tip: &Commit, checks: Vec<ResultKey>) {
    if !checks.is_empty()
        && let Err(error) = ctx.hub.tip_check_failures(&TipCheckFailures {
            commit: tip.clone(),
            checks,
        })
    {
        eprintln!("ignite: required failures at {tip} not recorded: {error}");
    }
}

/// The batches grouped by family, keeping their order.
fn by_family(batches: Vec<(String, u32, Vec<CheckName>)>) -> Vec<FamilyBatches> {
    let mut families: Vec<FamilyBatches> = Vec::new();
    for (family, batch, names) in batches {
        match families.last_mut() {
            Some((previous, numbered)) if *previous == family => numbered.push((batch, names)),
            _ => families.push((family, vec![(batch, names)])),
        }
    }
    families
}

/// The costs a round at `tip` cuts `family` by, as the hub fixed them for the tip. A hub that
/// cannot answer leaves the family cut as one nobody has measured, which every machine in the
/// same position cuts alike.
fn family_costs(ctx: &Context, tip: &Commit, family: &str) -> TipCosts {
    let claim_seconds = ctx.config.tip.claim_seconds;
    let request = TipCostsRequest {
        commit: tip.clone(),
        family: family.to_owned(),
        claim_seconds,
    };
    ctx.hub.tip_costs(&request).unwrap_or_else(|error| {
        eprintln!("ignite: tip verification: no costs for {family}, cutting by count: {error}");
        TipCosts::unmeasured(claim_seconds)
    })
}

/// The checks to run, in claimable batches. Each family that has a check to run is cut from
/// all its checks in `listed`, sorted by name, with the costs `costs` gives for it, so every
/// machine cuts the same batches; each batch keeps only the checks in `to_run`, and a batch
/// left empty is dropped with its number. Families with checks only this machine's platform
/// can run come first.
fn batches(
    listed: &[Listed],
    to_run: &[&TipCheck],
    costs: &dyn Fn(&str) -> TipCosts,
) -> Vec<(String, u32, Vec<CheckName>)> {
    let wanted: HashSet<&CheckName> = to_run.iter().map(|c| &c.check).collect();
    let working: HashSet<&str> = to_run.iter().map(|c| c.family.as_str()).collect();
    let mut families: BTreeMap<(bool, &str), Vec<CheckName>> = BTreeMap::new();
    for check in listed {
        if !working.contains(check.family.as_str()) {
            continue;
        }
        families
            .entry((check.portable, check.family.as_str()))
            .or_default()
            .push(check.check.clone());
    }
    let mut batches = Vec::new();
    for ((_, family), mut names) in families {
        names.sort();
        names.dedup();
        let family_costs = costs(family);
        for (index, cut) in tipround::cut(&names, &family_costs).into_iter().enumerate() {
            let batch = u32::try_from(index).unwrap_or(u32::MAX);
            let run: Vec<CheckName> = cut.into_iter().filter(|n| wanted.contains(n)).collect();
            if !run.is_empty() {
                batches.push((family.to_owned(), batch, run));
            }
        }
    }
    batches
}
