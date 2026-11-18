//! Running a round's checks: each family's batches from one listing, several families at once.
//!
//! A family's batches are cut from every check of the family the tip needs on this platform,
//! not only those this machine still has to run, with the costs the hub fixed for the tip
//! ([`ignite_core::tipround`]). So every machine cuts the same batches, and a batch number
//! names the same checks on each, however many of them another machine has run meanwhile. A
//! batch is then run for the checks in it that still need a run here, and one with none is not
//! claimed.
//!
//! The batches are claimed from the hub one at a time, right before each runs, so two machines
//! never run the same batch and a claim never lapses while its batch waits. Families run on up
//! to [`TipWork::parallel`] threads, each in its own landing tree; every command still asks the
//! scheduler for its cores in the `tip` class, so a busy machine runs fewer at once.

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread;

use ignite_core::ids::{CheckName, Commit};
use ignite_core::protocol::hub::TipClaim;
use ignite_core::tipround::{self, TipCosts, TipCostsRequest};
use ignite_core::traits::Result;

use super::{TipCheck, TipRun, TipVerifier};
use crate::server::Context;

/// One family's batches, in the order every machine cuts them.
type FamilyBatches = (String, Vec<(u32, Vec<CheckName>)>);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What the threads of one round share.
struct Round<'a> {
    ctx: &'a Context,
    tip: &'a Commit,
    queue: Mutex<VecDeque<FamilyBatches>>,
    /// How many families have been started.
    started: Mutex<usize>,
    ran_a_batch: AtomicBool,
    /// A newer tip that ended the round, or the error reading `staging`.
    ended: Mutex<Option<Result<Commit>>>,
    runs: Mutex<Vec<TipRun>>,
}

impl Round<'_> {
    /// The next family to run, or `None` when none is left or the round has ended. Past the
    /// first family, while no batch has run yet, a newer `staging` tip ends the round.
    fn next(&self) -> Option<FamilyBatches> {
        let mut queue = lock(&self.queue);
        if lock(&self.ended).is_some() {
            return None;
        }
        let next = queue.pop_front()?;
        let mut started = lock(&self.started);
        if *started > 0 && !self.ran_a_batch.load(Ordering::SeqCst) {
            match self.ctx.git.tip(&self.ctx.config.repository.staging) {
                Ok(newest) if newest == *self.tip => {}
                other => {
                    *lock(&self.ended) = Some(other);
                    return None;
                }
            }
        }
        *started += 1;
        Some(next)
    }

    /// Claims one batch from the hub. A claim the hub cannot answer is not granted, so the
    /// batch is left for a later round.
    fn claim(&self, family: &str, batch: u32) -> bool {
        let claim = TipClaim {
            commit: self.tip.clone(),
            family: family.to_owned(),
            batch,
            machine: self.ctx.machine.clone(),
        };
        let granted = match self.ctx.hub.tip_claim(&claim) {
            Ok(reply) => reply.granted,
            Err(error) => {
                eprintln!(
                    "ignite: tip verification: batch {batch} of {family} not claimed: {error}"
                );
                false
            }
        };
        if granted {
            self.ran_a_batch.store(true, Ordering::SeqCst);
        }
        granted
    }
}

impl TipVerifier {
    /// Runs `to_run`, some of the tip's `checks`, at `tip` and returns the runs. While no batch has run yet (another machine
    /// holds them all, say), a newer `staging` tip ends the round and is returned instead, so
    /// the next round verifies it. A round that has run a batch finishes, so a shared input
    /// that every landing changes cannot keep any tip from being verified.
    pub(super) fn run_families(
        &self,
        ctx: &Context,
        tip: &Commit,
        checks: &[TipCheck],
        to_run: &[&TipCheck],
    ) -> Result<(Option<Commit>, Vec<TipRun>)> {
        let costs = |family: &str| family_costs(ctx, tip, family);
        let round = Round {
            ctx,
            tip,
            queue: Mutex::new(by_family(batches(checks, to_run, &costs)).into()),
            started: Mutex::new(0),
            ran_a_batch: AtomicBool::new(false),
            ended: Mutex::new(None),
            runs: Mutex::new(Vec::new()),
        };
        // Each family's thread works for the round's activity, so its builds run in the `tip`
        // class and `ignite status` shows which family it is on.
        let activity = crate::steps::current();
        thread::scope(|scope| {
            for _ in 0..self.work.parallel().max(1) {
                scope.spawn(|| {
                    let _entered = activity.as_ref().map(crate::steps::Activity::enter);
                    while let Some((family, batches)) = round.next() {
                        crate::steps::step(format!("verifying family {family}"));
                        let claim = |batch: u32| round.claim(&family, batch);
                        let (runs, _) = self.work.run_batches(ctx, tip, &family, &batches, &claim);
                        lock(&round.runs).extend(runs);
                    }
                });
            }
        });
        let superseded = round
            .ended
            .into_inner()
            .unwrap_or_else(PoisonError::into_inner)
            .transpose()?;
        let runs = round
            .runs
            .into_inner()
            .unwrap_or_else(PoisonError::into_inner);
        Ok((superseded, runs))
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
/// all its runnable checks in `checks`, sorted by name, with the costs `costs` gives for it, so
/// every machine cuts the same batches; each batch keeps only the checks in `to_run`, and a
/// batch left empty is dropped with its number. Families with checks only this machine's
/// platform can run come first.
fn batches(
    checks: &[TipCheck],
    to_run: &[&TipCheck],
    costs: &dyn Fn(&str) -> TipCosts,
) -> Vec<(String, u32, Vec<CheckName>)> {
    let wanted: HashSet<&CheckName> = to_run.iter().map(|c| &c.check).collect();
    let working: HashSet<&str> = to_run.iter().map(|c| c.family.as_str()).collect();
    let mut families: BTreeMap<(bool, &str), Vec<CheckName>> = BTreeMap::new();
    for check in checks {
        if !check.runnable || !working.contains(check.family.as_str()) {
            continue;
        }
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
