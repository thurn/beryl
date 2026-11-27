//! A round of tip verification as the internal verb `tip-round`, run in a verb process of its
//! own, and the tip loop that starts each round and waits for it.
//!
//! Before, the daemon's tip loop ran each round on its own thread, so a restart of the daemon
//! into a new build cut the round off: its commands in landing trees were ended, its claims at
//! the hub lapsed, and the next daemon started the round over. Now:
//!
//! - The loop ([`TipLoop`]) waits on the hub's change feed in the daemon, asks the verifier's
//!   saved memory whether the shared candidate needs a round, and
//!   starts the round with [`Server::start_internal`], which gives it a handle and starts
//!   `ignite internal verb <handle>` (see `server/process.rs`). It then waits on the handle.
//! - The round ([`TipRoundVerb`]) loads the verifier's memory from the local database
//!   (`tip_state`, see `state`), verifies the tip with [`TipVerifier::round_at`], saves the
//!   memory and answers with the tip and its [`TipOutcome`]. Its commands, builds and landing
//!   trees are asked of the daemon through the host protocol (see `crate::host`), and the trees
//!   it leases are owned by its handle, so a restarted daemon keeps them. It renews its own
//!   claims at the hub (see `families`) and moves `master` itself.
//! - A restarted daemon finds the round's verb process alive by its lock and goes on serving
//!   it. Its tip loop finds the running `tip-round` handle ([`Server::running_internal`]) and
//!   waits for it instead of starting another round.
//! - When a round's reply says `master` moved, the loop asks the daemon's self-update to build
//!   the new release (`SelfUpdate::after_master_moved`), as before; a loop that waited for a
//!   round started before a restart does the same with that round's reply.
//! - While waiting, the loop checks whether a failed candidate can give way to newer staging,
//!   the hub already selected a replacement, or `master` advanced past its target. A round
//!   that has recorded its failed verdict finishes its culprit search whatever newer tip
//!   appears, and stops only when `master` passes its target or all failed keys pass, so
//!   ordinary verification can resume. A round still running its own checks before `master`
//!   moves gets a bounded grace window when a newer candidate appears, and yields immediately
//!   when `master` reaches its target or a revert replaces it.
//!
//! A round whose verb process dies is not run again: its handle ends `incomplete`, and the next
//! turn of the loop starts a new round when the tip still needs one.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use ignite_core::ids::{Commit, Handle, Seq};
use ignite_core::protocol::Class;
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Error, Result};

use super::{TipOutcome, TipVerifier, TipWork, candidate, families};
use crate::handles::Waited;
use crate::server::{Context, Server, Verb, error_reply};

/// The internal verb that runs one round of tip verification.
pub const ROUND_VERB: &str = "tip-round";

/// How long the loop waits on a round's handle between checks for supersession or recovery.
const WAIT_S: u32 = 60;

/// What a round answers with, in its reply's JSON.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoundReply {
    pub tip: Commit,
    pub outcome: TipOutcome,
}

/// One round of tip verification at the tip and feed position its request names (see the
/// module documentation).
pub struct TipRoundVerb {
    pub work: Arc<dyn TipWork>,
    /// The Ignite commit that records name.
    pub ignite: String,
    /// This machine's hostname, which the hub's tickets name.
    pub host: String,
    /// The local database that holds the verifier's memory; `None` keeps none.
    pub memory: Option<PathBuf>,
    /// How often a running batch's claim is renewed.
    pub renew_every: Duration,
}

impl TipRoundVerb {
    pub fn new(
        work: Arc<dyn TipWork>,
        ignite: impl Into<String>,
        host: impl Into<String>,
        memory: Option<PathBuf>,
    ) -> TipRoundVerb {
        TipRoundVerb {
            work,
            ignite: ignite.into(),
            host: host.into(),
            memory,
            renew_every: families::RENEW_EVERY,
        }
    }

    /// A verifier with the memory the last round saved.
    pub fn verifier(&self) -> TipVerifier {
        let verifier = TipVerifier::new(self.work.clone(), self.ignite.clone())
            .with_host(self.host.clone())
            .renewing_claims_every(self.renew_every);
        let Some(path) = &self.memory else {
            return verifier;
        };
        match ignite_store::local::open(path) {
            Ok(conn) => verifier.with_memory(conn),
            Err(error) => {
                eprintln!("ignite: the tip verifier's memory is not kept: {error}");
                verifier
            }
        }
    }

    /// The request of a round at `tip`, started at feed position `since`, from `cwd`.
    pub fn request(tip: &Commit, since: Seq, cwd: &Path) -> Request {
        Request {
            verb: ROUND_VERB.to_owned(),
            args: vec![tip.to_string(), since.0.to_string()],
            cwd: cwd.to_path_buf(),
            session: None,
            wait_s: 0,
            pid: None,
        }
    }

    fn parse(req: &Request) -> Result<(Commit, Seq)> {
        let invalid = || Error::Invalid(format!("`{ROUND_VERB}` takes a commit and a position"));
        let [tip, since] = req.args.as_slice() else {
            return Err(invalid());
        };
        let tip = Commit::new(tip).map_err(|_| invalid())?;
        let since = since.parse::<u64>().map_err(|_| invalid())?;
        Ok((tip, Seq(since)))
    }
}

impl Verb for TipRoundVerb {
    fn name(&self) -> &str {
        ROUND_VERB
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        // Its builds run in the `tip` class, as the tip loop's did.
        crate::steps::set_class(Class::Tip);
        let (tip, since) = match Self::parse(req) {
            Ok(parsed) => parsed,
            Err(error) => return error_reply(&error),
        };
        match self.verifier().round_at(ctx, &tip, since) {
            Ok(outcome) => {
                let text = format!("tip {tip}: {outcome:?}");
                let json = serde_json::to_value(RoundReply { tip, outcome }).ok();
                Reply {
                    exit: Exit::Success,
                    text,
                    json,
                    handle: None,
                }
            }
            Err(error) => error_reply(&error),
        }
    }
}

/// The tip loop's side: waits on the feed, starts each round in a verb process and waits for
/// it (see the module documentation).
pub struct TipLoop {
    pub server: Arc<Server>,
    /// The round, which runs on a thread of the daemon when no verb process can be started.
    pub verb: Arc<TipRoundVerb>,
    /// The feed position already seen.
    since: Seq,
    /// Whether to check for new work without waiting on the feed.
    retry_now: bool,
    /// How long to wait on a round's handle between checks for supersession or recovery.
    poll_every: Duration,
}

impl TipLoop {
    pub fn new(server: Arc<Server>, verb: Arc<TipRoundVerb>) -> TipLoop {
        let since = verb.verifier().since();
        TipLoop {
            server,
            verb,
            since,
            retry_now: false,
            poll_every: Duration::from_secs(u64::from(WAIT_S)),
        }
    }

    #[cfg(test)]
    pub(crate) fn poll_every(mut self, every: Duration) -> Self {
        self.poll_every = every;
        self
    }

    /// One turn of the loop: waits for a round still running from before a restart, or else
    /// waits up to `wait_s` for the feed to show a change and runs a round at the shared
    /// candidate if it needs one. Returns the tip a round verified and how it ended, or
    /// `None` when there was nothing to verify.
    pub fn turn(&mut self, ctx: &Context, wait_s: u32) -> Result<Option<(Commit, TipOutcome)>> {
        if let Some((handle, request)) = self.server.running_internal(ROUND_VERB) {
            crate::steps::step(format!("waiting for tip round {handle}"));
            let target = request.and_then(|request| TipRoundVerb::parse(&request).ok());
            return self
                .wait(ctx, &handle, target.as_ref().map(|(tip, _)| tip), Duration::ZERO)
                .map(Some);
        }
        crate::steps::step("waiting for a tip to verify");
        let batch = ctx
            .hub
            .feed(self.since, if self.retry_now { 0 } else { wait_s })?;
        self.since = self.since.max(batch.seq);
        let Some(tip) = self.verb.verifier().next_target(ctx)? else {
            self.retry_now = false;
            return Ok(None);
        };
        let request = TipRoundVerb::request(&tip, self.since, &ctx.repo);
        let handle = self.server.start_internal(self.verb.clone(), request);
        crate::steps::step(format!("verifying tip {tip} in {handle}"));
        self.wait(ctx, &handle, Some(&tip), self.poll_every)
            .map(Some)
    }

    /// Waits for the round and reads its reply. An adopted round can already be obsolete,
    /// so its first wait only checks whether it has finished.
    fn wait(
        &mut self,
        ctx: &Context,
        handle: &Handle,
        target: Option<&Commit>,
        mut wait: Duration,
    ) -> Result<(Commit, TipOutcome)> {
        let handles = self.server.handles();
        let mut interrupted = None;
        let mut superseded_since = None;
        let reply = loop {
            match handles.wait_for(handle, wait, None) {
                Waited::Running(_) => {
                    wait = self.poll_every;
                    if interrupted.is_none()
                        && let Some(tip) = target
                        && let Some(outcome) =
                            self.interruption(ctx, tip, handle, &mut superseded_since)
                        && self.server.stop_internal(handle)
                    {
                        interrupted = Some((tip.clone(), outcome));
                    }
                }
                Waited::Done(reply) | Waited::Interrupted(reply) => break reply,
                Waited::Unknown => {
                    return Err(Error::NotFound(format!("the tip round {handle}")));
                }
            }
        };
        handles.delivered(handle);
        let read = reply
            .json
            .clone()
            .and_then(|json| serde_json::from_value::<RoundReply>(json).ok());
        let finished = match read {
            Some(RoundReply { tip, outcome }) => Ok((tip, outcome)),
            None if let Some(interrupted) = interrupted => Ok(interrupted),
            None => Err(Error::Conflict(format!("{handle}: {}", reply.text))),
        };
        self.retry_now = matches!(
            finished,
            Ok((
                _,
                TipOutcome::Failed { .. } | TipOutcome::Recovered | TipOutcome::Superseded { .. }
            ))
        );
        finished
    }

    /// Why the loop stops the running round, if it should.
    ///
    /// A newer candidate does not stop a round that has recorded its failed verdict and is
    /// searching for culprits. On 2026-11-25 landings arrived every 10 to 20 minutes while
    /// rounds took 30 or more, so the newer candidate that stopped each search also failed and
    /// was stopped in turn. For 16 hours no search finished: the hub filed regressions with no
    /// culprit, nothing was reverted, and `promote_ancestor` never moved `master` to the
    /// commits before the culprits, while 75 product commits waited. The search's range ends
    /// at the failed tip, so a newer tip does not change its answer, and a revert is built on
    /// the newest `staging` anyway. `master` reaching the tip, or the failed checks passing,
    /// still ends the search, since neither its culprits nor its revert would then be needed.
    ///
    /// A round still running its own checks before `master` moves gets a bounded grace window
    /// when a newer candidate appears, and yields immediately when `master` reaches its target
    /// or a revert replaces it, for the reason given at `candidate::ends_checks`.
    fn interruption(
        &self,
        ctx: &Context,
        tip: &Commit,
        handle: &Handle,
        superseded_since: &mut Option<Timestamp>,
    ) -> Option<TipOutcome> {
        let verifier = self.verb.verifier();
        if let Some(by) = self.superseding_tip(ctx, tip) {
            let is_checking = checking(&verifier, tip, handle);
            let grace_done =
                is_checking && candidate::checking_superseded(ctx, &by, superseded_since);
            if !is_checking {
                *superseded_since = None;
            }
            if master_reached(ctx, tip).is_some()
                || (!searching(&verifier, tip, handle) && (!is_checking || grace_done))
            {
                return Some(TipOutcome::Superseded { by });
            }
        } else {
            *superseded_since = None;
        }
        verifier
            .failure_recovered(ctx, tip, Some(handle))
            .then_some(TipOutcome::Recovered)
    }

    /// A round can spend hours waiting for checks or searching for a culprit. A running
    /// loop can select the successor itself once the hub lets the candidate advance.
    fn superseding_tip(&self, ctx: &Context, target: &Commit) -> Option<Commit> {
        if let Some(master) = master_reached(ctx, target) {
            return (master != *target).then_some(master);
        }
        let selected = ctx.hub.tip_candidate().ok()??;
        if selected.commit != *target {
            return Some(selected.commit);
        }
        if !selected.retry_on_new_tip {
            return None;
        }
        self.verb
            .verifier()
            .next_target(ctx)
            .ok()
            .flatten()
            .filter(|tip| tip != target)
    }
}

/// `master` when it has reached `target` or moved past it.
pub(super) fn master_reached(ctx: &Context, target: &Commit) -> Option<Commit> {
    ctx.git
        .tip(&ctx.config.repository.master)
        .ok()
        .filter(|master| master == target || ctx.git.is_ancestor(target, master).unwrap_or(false))
}

/// Whether the round `handle` has recorded a failed verdict at `tip` and gone on to search for
/// culprits, revert and promote an ancestor (see `verify`, which saves the failure first).
fn searching(verifier: &TipVerifier, tip: &Commit, handle: &Handle) -> bool {
    verifier
        .failed
        .as_ref()
        .is_some_and(|failed| failed.commit == *tip && failed.round.as_ref() == Some(handle))
}

/// Whether the round `handle` is running its own checks at `tip` before `master` moves (see
/// `verify`, which saves this before the round starts them).
fn checking(verifier: &TipVerifier, tip: &Commit, handle: &Handle) -> bool {
    verifier
        .checking
        .as_ref()
        .is_some_and(|at| at.commit == *tip && at.round.as_ref() == Some(handle))
}

/// Leaves a stopped round before it treats interrupted checks as missing or failing.
pub(super) fn require_running() -> Result<()> {
    if crate::steps::stop_requested() {
        return Err(Error::Conflict("the tip round was stopped".to_owned()));
    }
    Ok(())
}
