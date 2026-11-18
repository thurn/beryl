//! Running a round's checks: each family's batches from one listing, several families at once.
//!
//! The batches are claimed from the hub one at a time, right before each runs, so two machines
//! never run the same batch and a claim never lapses while its batch waits. Families run on up
//! to [`TipWork::parallel`] threads, each in its own landing tree; every command still asks the
//! scheduler for its cores in the `tip` class, so a busy machine runs fewer at once.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread;

use ignite_core::ids::{CheckName, Commit};
use ignite_core::protocol::hub::TipClaim;
use ignite_core::traits::Result;

use super::{BATCH, TipCheck, TipRun, TipVerifier};
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
    /// Runs `to_run` at `tip` and returns the runs. While no batch has run yet (another machine
    /// holds them all, say), a newer `staging` tip ends the round and is returned instead, so
    /// the next round verifies it. A round that has run a batch finishes, so a shared input
    /// that every landing changes cannot keep any tip from being verified.
    pub(super) fn run_families(
        &self,
        ctx: &Context,
        tip: &Commit,
        to_run: &[&TipCheck],
    ) -> Result<(Option<Commit>, Vec<TipRun>)> {
        let round = Round {
            ctx,
            tip,
            queue: Mutex::new(by_family(batches(to_run)).into()),
            started: Mutex::new(0),
            ran_a_batch: AtomicBool::new(false),
            ended: Mutex::new(None),
            runs: Mutex::new(Vec::new()),
        };
        thread::scope(|scope| {
            for _ in 0..self.work.parallel().max(1) {
                scope.spawn(|| {
                    while let Some((family, batches)) = round.next() {
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
