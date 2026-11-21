//! A round of tip verification as the internal verb `tip-round`, run in a verb process of its
//! own, and the tip loop that starts each round and waits for it.
//!
//! Before, the daemon's tip loop ran each round on its own thread, so a restart of the daemon
//! into a new build cut the round off: its commands in landing trees were ended, its claims at
//! the hub lapsed, and the next daemon started the round over. Now:
//!
//! - The loop ([`TipLoop`]) waits on the hub's change feed in the daemon, asks the verifier's
//!   saved memory whether the newest `staging` tip needs a round ([`TipVerifier::due`]), and
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
use ignite_core::traits::{Error, Result};

use super::{TipOutcome, TipVerifier, TipWork, families};
use crate::handles::Waited;
use crate::server::{Context, Server, Verb, error_reply};

/// The internal verb that runs one round of tip verification.
pub const ROUND_VERB: &str = "tip-round";

/// How long one wait on a round's handle lasts before the loop waits again; the loop's step
/// stays the same throughout.
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
}

impl TipLoop {
    pub fn new(server: Arc<Server>, verb: Arc<TipRoundVerb>) -> TipLoop {
        let since = verb.verifier().since();
        TipLoop {
            server,
            verb,
            since,
        }
    }

    /// One turn of the loop: waits for a round still running from before a restart, or else
    /// waits up to `wait_s` for the feed to show a change and runs a round at the newest
    /// `staging` tip if it needs one. Runs `before_start` only before starting a new round.
    /// That gives the floor sweep a chance to reclaim released build outputs before the new
    /// round leases their tree again. Returns the tip a round verified and how it ended, or
    /// `None` when there was nothing to verify.
    pub fn turn(
        &mut self,
        ctx: &Context,
        wait_s: u32,
        before_start: impl FnOnce(),
    ) -> Result<Option<(Commit, TipOutcome)>> {
        if let Some(handle) = self.server.running_internal(ROUND_VERB) {
            crate::steps::step(format!("waiting for tip round {handle}"));
            return self.wait(&handle).map(Some);
        }
        crate::steps::step("waiting for a new tip, then verifying it");
        let batch = ctx.hub.feed(self.since, wait_s)?;
        self.since = self.since.max(batch.seq);
        let tip = ctx.git.tip(&ctx.config.repository.staging)?;
        if !self.verb.verifier().due(ctx, &tip) {
            return Ok(None);
        }
        before_start();
        let request = TipRoundVerb::request(&tip, self.since, &ctx.repo);
        let handle = self.server.start_internal(self.verb.clone(), request);
        crate::steps::step(format!("verifying tip {tip} in {handle}"));
        self.wait(&handle).map(Some)
    }

    /// Waits for the round `handle` to finish and reads its reply.
    fn wait(&self, handle: &Handle) -> Result<(Commit, TipOutcome)> {
        let handles = self.server.handles();
        let reply = loop {
            match handles.wait(handle, WAIT_S, None) {
                Waited::Running(_) => {}
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
        match read {
            Some(RoundReply { tip, outcome }) => Ok((tip, outcome)),
            None => Err(Error::Conflict(format!("{handle}: {}", reply.text))),
        }
    }
}
