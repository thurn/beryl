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

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use ignite_core::ids::{CheckName, Commit};
use ignite_core::protocol::hub::TipClaim;
use ignite_core::tipround::{self, TipCosts, TipCostsRequest};
use ignite_core::traits::Result;

use super::{Listed, TipCheck, TipRun, TipVerifier};
use crate::server::Context;

/// How often a running batch's claim is renewed: a third of the hub's claim
/// ([`ignite_store::hub::tips::CLAIM_SECONDS`]), so two renewals can fail before it lapses.
pub const RENEW_EVERY: Duration = Duration::from_secs(300);

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
    /// How many families a thread is working on now: building, listing or running them.
    in_flight: AtomicUsize,
    ran_a_batch: AtomicBool,
    /// A newer tip that ended the round, or the error reading `staging`.
    ended: Mutex<Option<Result<Commit>>>,
    runs: Mutex<Vec<TipRun>>,
    /// The batch each family is running under a claim, which the renewer claims again.
    held: Mutex<BTreeMap<String, u32>>,
}

impl Round<'_> {
    /// The next family to run, or `None` when none is left or the round has ended. Past the
    /// first family, a newer `staging` tip ends the round only while the round has nothing
    /// under way: no batch has run and no other family is being built, listed or run. A family
    /// builds and lists before it claims its first batch, which can take ten minutes under
    /// load, so ending the round then would throw that work away, and with landings arriving
    /// more often than that, `master` would never move.
    fn next(&self) -> Option<FamilyBatches> {
        let mut queue = lock(&self.queue);
        if lock(&self.ended).is_some() {
            return None;
        }
        let next = queue.pop_front()?;
        let mut started = lock(&self.started);
        if *started > 0
            && !self.ran_a_batch.load(Ordering::SeqCst)
            && self.in_flight.load(Ordering::SeqCst) == 0
        {
            match self.ctx.git.tip(&self.ctx.config.repository.staging) {
                Ok(newest) if newest == *self.tip => {}
                other => {
                    *lock(&self.ended) = Some(other);
                    return None;
                }
            }
        }
        *started += 1;
        self.in_flight.fetch_add(1, Ordering::SeqCst);
        Some(next)
    }

    /// Claims one batch from the hub. A claim the hub cannot answer is not granted, so the
    /// batch is left for a later round.
    fn claim(&self, family: &str, batch: u32) -> bool {
        // A family asks for its next batch once the last one has finished.
        lock(&self.held).remove(family);
        let granted = self.ask(family, batch);
        if granted {
            self.ran_a_batch.store(true, Ordering::SeqCst);
            lock(&self.held).insert(family.to_owned(), batch);
        }
        granted
    }

    /// Asks the hub for `batch` of `family`. The hub grants a batch this machine holds again,
    /// renewing its claim.
    fn ask(&self, family: &str, batch: u32) -> bool {
        let claim = TipClaim {
            commit: self.tip.clone(),
            family: family.to_owned(),
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
    /// over the round's listing. While nothing is under way (another machine holds every batch
    /// so far, say), a newer `staging` tip ends the round and is returned instead, so the next
    /// round verifies it. A round that has run a batch, or is building or listing a family,
    /// finishes, so landings that keep arriving cannot keep every tip from being verified.
    pub(super) fn run_families(
        &self,
        ctx: &Context,
        tip: &Commit,
        to_run: &[&TipCheck],
    ) -> Result<(Option<Commit>, Vec<TipRun>)> {
        let costs = |family: &str| family_costs(ctx, tip, family);
        let round = Round {
            ctx,
            tip,
            queue: Mutex::new(by_family(batches(&self.listed, to_run, &costs)).into()),
            started: Mutex::new(0),
            in_flight: AtomicUsize::new(0),
            ran_a_batch: AtomicBool::new(false),
            ended: Mutex::new(None),
            runs: Mutex::new(Vec::new()),
            held: Mutex::new(BTreeMap::new()),
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
                            crate::steps::step(format!("verifying family {family}"));
                            let claim = |batch: u32| round.claim(&family, batch);
                            let (runs, _) =
                                self.work.run_batches(ctx, tip, &family, &batches, &claim);
                            lock(&round.held).remove(&family);
                            lock(&round.runs).extend(runs);
                            round.in_flight.fetch_sub(1, Ordering::SeqCst);
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
