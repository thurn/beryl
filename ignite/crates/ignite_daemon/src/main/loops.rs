//! The daemon's background threads, and the hub that the hub's machine serves beside its
//! daemon. Each loop reports a failed round on standard error and tries again, so an
//! unreachable hub or a busy tree never stops the daemon.

use std::cell::RefCell;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use rusqlite::Connection;

use ignite_core::protocol::Class;
use ignite_core::protocol::hub::Heartbeat;
use ignite_core::time::SystemClock;
use ignite_core::traits::{Error, Result};

use super::DaemonOptions;
use super::selfupdate::SelfUpdate;
use crate::checks::Checks;
use crate::claim::Claims;
use crate::crash;
use crate::dispatchd::Dispatcher;
use crate::hubclient::HubClient;
use crate::idle::Idle;
use crate::land::Landing;
use crate::loop_health::{LoopHealth, LoopTicker};
use crate::sched::Scheduler;
use crate::server::{Context, Server};
use crate::supervise::SupervisedLauncher;
use crate::tip::{CheckTipWork, TipOutcome, TipVerifier};
use crate::trees::Trees;
use crate::update::Home;

/// How often the outbox is sent and the heartbeat beats.
const SEND_EVERY: Duration = Duration::from_secs(10);
/// How often the stall check looks at held tickets.
const TICK_EVERY: Duration = Duration::from_secs(60);
/// How often idle work looks for cores nobody uses.
const IDLE_EVERY: Duration = Duration::from_secs(30);
/// How often fleet metrics look for new session transcripts.
const METRICS_EVERY: Duration = Duration::from_secs(600);
/// The pause after a failed round of tip verification.
const TIP_RETRY: Duration = Duration::from_secs(10);

/// How long each loop may go without finishing a round before `ignite doctor` calls it stuck.
/// Each is far longer than the loop's own period, so that a slow hub or a long job is not
/// reported, and only a loop that has stopped is. A round of tip verification may run for an
/// hour, because it runs the checks.
const SENDER_STALLS_AFTER: Duration = Duration::from_mins(10);
const TIP_STALLS_AFTER: Duration = Duration::from_hours(3);
const CLAIMS_STALL_AFTER: Duration = Duration::from_mins(10);
const IDLE_STALLS_AFTER: Duration = Duration::from_mins(10);
const METRICS_STALL_AFTER: Duration = Duration::from_hours(1);

/// What the loops share, built by [`super::Daemon::start`].
pub struct Parts {
    pub ctx: Context,
    pub client: Arc<HubClient>,
    pub trees: Trees,
    pub checks: Arc<Checks>,
    pub landing: Arc<Landing>,
    pub idle: Arc<Idle>,
    pub claims: Arc<Claims>,
    pub dispatcher: Arc<Dispatcher>,
    pub scheduler: Arc<Scheduler>,
    /// The sender's own connection to the local database, taken by the first [`Parts::spawn`].
    pub sender_db: Mutex<Option<Connection>>,
    pub home: Home,
    pub version: String,
    /// The machine's hostname, which names it in the hub's tickets.
    pub hostname: String,
    pub feed_wait_s: u32,
    pub update: Arc<SelfUpdate>,
    /// What each background loop has done lately, for `status` and `doctor`.
    pub health: LoopHealth,
    /// Sends crash records and stuck loops to the hub as justiciar reports.
    pub reporter: Arc<crate::justiciar::Reporter>,
}

impl Parts {
    /// Starts every loop. A second call starts nothing. `server` reports whether a verb is in a
    /// part that may not be cut short, which a restart waits for.
    pub fn spawn(&self, server: &Arc<Server>) {
        let Some(db) = self
            .sender_db
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        else {
            return;
        };
        self.spawn_sender(db, server.clone());
        self.spawn_tip();
        self.spawn_claims();
        self.spawn_idle();
        self.spawn_metrics();
    }

    /// Sends the outbox, the heartbeat and any landing the hub has not heard of. After each
    /// heartbeat it compares the daemon's version with the hub's and restarts into a new
    /// executable as soon as one is ready, whatever runs (see [`SelfUpdate::after_heartbeat`]).
    fn spawn_sender(&self, db: Connection, server: Arc<Server>) {
        let (ctx, client, landing, dispatcher, version) = (
            self.ctx.clone(),
            self.client.clone(),
            self.landing.clone(),
            self.dispatcher.clone(),
            self.version.clone(),
        );
        let update = self.update.clone();
        let reporter = self.reporter.clone();
        self.start_loop("ignite-sender", SENDER_STALLS_AFTER, move |ticker| {
            // The hub's version from the last heartbeat it answered.
            let hub_version: RefCell<Option<String>> = RefCell::new(None);
            let beat = || {
                let reply = client.heartbeat(&Heartbeat {
                    machine: ctx.machine.clone(),
                    version: version.clone(),
                    workers: dispatcher.view().unwrap_or_default(),
                })?;
                *hub_version.borrow_mut() = Some(reply.hub_version);
                Ok(())
            };
            loop {
                ticker.round(|| {
                    let sent = crate::outbox::tick(&db, ctx.hub.as_ref(), &beat);
                    landing.deliver_pending(&ctx);
                    if let Err(error) = update.poll_release() {
                        eprintln!("ignite: the release branch was not checked: {error}");
                    }
                    let critical = server.steps().critical_now();
                    update.after_heartbeat(hub_version.borrow().as_deref(), critical);
                    reporter.tick(&ctx, sent.is_ok());
                    sent
                });
                thread::sleep(SEND_EVERY);
            }
        });
    }

    /// Starts the loop `name` on a thread of that name. It registers with the loop health,
    /// which `body` reports each round to, and the daemon exits if it ever stops (see
    /// [`crash::spawn_loop`]).
    fn start_loop(
        &self,
        name: &'static str,
        stalls_after: Duration,
        body: impl FnOnce(LoopTicker) + Send + 'static,
    ) {
        let mut ticker = self.health.register(name, stalls_after);
        if let Some(class) = loop_class(name) {
            ticker = ticker.with_class(class);
        }
        crash::spawn_loop(name, move || body(ticker));
    }

    /// Verifies each new tip of the integration branch, then keeps the main checkout's
    /// branches fast-forwarded.
    fn verifier(&self) -> TipVerifier {
        let work = CheckTipWork {
            checks: self.checks.clone(),
            trees: Arc::new(self.trees.clone()),
        };
        TipVerifier::new(Arc::new(work), self.version.clone()).with_host(self.hostname.clone())
    }

    /// See [`super::Daemon::verify_tip`].
    pub fn verify_tip(&self) -> Result<TipOutcome> {
        let tip = self.ctx.git.tip(&self.ctx.config.repository.staging)?;
        self.verifier().verify(&self.ctx, &tip)
    }

    fn spawn_tip(&self) {
        let mut verifier = self.verifier();
        let (ctx, trees, wait_s) = (self.ctx.clone(), self.trees.clone(), self.feed_wait_s);
        let update = self.update.clone();
        self.start_loop("ignite-tip", TIP_STALLS_AFTER, move |ticker| {
            loop {
                let round = ticker.round(|| {
                    crate::steps::step("waiting for a new tip, then verifying it");
                    match verifier.round(&ctx, wait_s) {
                        Ok(Some((_, TipOutcome::Verified))) => {
                            if let Err(error) = update.after_master_moved() {
                                eprintln!("ignite: the new release was not built: {error}");
                            }
                            Ok(())
                        }
                        Ok(_) => Ok(()),
                        Err(error) => {
                            eprintln!("ignite: tip verification: {error}");
                            Err(format!("tip verification: {error}"))
                        }
                    }
                });
                if round.is_none() {
                    thread::sleep(TIP_RETRY);
                }
                if let Err(error) = trees.fast_forward_main() {
                    eprintln!("ignite: main checkout not fast-forwarded: {error}");
                }
            }
        });
    }

    fn spawn_claims(&self) {
        let (ctx, claims) = (self.ctx.clone(), self.claims.clone());
        self.start_loop("ignite-claims", CLAIMS_STALL_AFTER, move |ticker| {
            loop {
                thread::sleep(TICK_EVERY);
                ticker.round(|| {
                    claims.tick(&ctx);
                    Ok(())
                });
            }
        });
    }

    /// While Ignite holds no cores and outside work leaves at least half the machine free,
    /// keeps one free tree built at the tip and runs one piece of idle work.
    fn spawn_idle(&self) {
        let (ctx, idle, trees, scheduler) = (
            self.ctx.clone(),
            self.idle.clone(),
            self.trees.clone(),
            self.scheduler.clone(),
        );
        self.start_loop("ignite-idle", IDLE_STALLS_AFTER, move |ticker| {
            loop {
                thread::sleep(IDLE_EVERY);
                ticker.round(|| {
                    let status = scheduler.status();
                    if status.granted > 0 || status.outside * 2 >= status.cores {
                        return Ok(());
                    }
                    let mut failure = None;
                    crate::steps::step("keeping a free tree built at the tip");
                    if let Ok(tip) = ctx.git.tip(&ctx.config.repository.staging)
                        && let Err(error) = trees.keep_one_built(&tip)
                    {
                        eprintln!("ignite: no tree kept built: {error}");
                        failure = Some(format!("no tree kept built: {error}"));
                    }
                    crate::steps::step("choosing idle work");
                    if let Some(job) = idle.next_job(&ctx)
                        && let Err(error) = {
                            crate::steps::step(format!("idle work {job:?}"));
                            idle.run(&ctx, &job)
                        }
                    {
                        eprintln!("ignite: idle work {job:?}: {error}");
                        failure = Some(format!("idle work {job:?}: {error}"));
                    }
                    failure.map_or(Ok(()), Err)
                });
            }
        });
    }

    fn spawn_metrics(&self) {
        let (ctx, state) = (self.ctx.clone(), self.home.root.join("metrics.json"));
        self.start_loop("ignite-metrics", METRICS_STALL_AFTER, move |ticker| {
            loop {
                ticker.round(|| {
                    crate::metrics::post_new_rows(&ctx, &state).map_err(|error| {
                        eprintln!("ignite: metrics not posted: {error}");
                        format!("metrics not posted: {error}")
                    })
                });
                thread::sleep(METRICS_EVERY);
            }
        });
    }
}

/// The class of the work a loop's rounds do. Builds a round starts run in it, so tip
/// verification's builds are `tip` work and the free tree the idle loop keeps built is built as
/// `idle` work, which yields to everything else. Before, both ran as `agent` work: the tip and
/// idle loops sat in rounds for over half an hour while `ignite status --admission` showed `tip`
/// and `idle` with nothing running, their builds queued behind the agents' own.
fn loop_class(name: &str) -> Option<Class> {
    match name {
        "ignite-tip" => Some(Class::Tip),
        "ignite-idle" => Some(Class::Idle),
        _ => None,
    }
}

/// Takes over the runs a stopped daemon left: each is registered with the scheduler under the
/// cores and resources it was granted, so nothing else is granted them while it runs, and
/// released when its supervisor has exited.
pub fn collect_unfinished(launcher: &SupervisedLauncher) {
    launcher.adopt_unfinished();
}

/// On the hub's machine (`IGNITE_HUB_LISTEN` set), serves the hub and runs its upkeep for the
/// life of the process.
pub fn serve_hub_if_asked(options: &DaemonOptions) -> Result<()> {
    let Some(listen) = std::env::var_os("IGNITE_HUB_LISTEN") else {
        return Ok(());
    };
    let dir = options.home.root.join("hub");
    std::fs::create_dir_all(&dir)?;
    serve_hub(
        &options.repo,
        &listen.to_string_lossy(),
        &dir,
        &options.version,
    )
}

/// Serves the hub's database in `dir` at `listen` and starts its upkeep thread.
pub fn serve_hub(repo: &Path, listen: &str, dir: &Path, version: &str) -> Result<()> {
    let config = super::read_config(repo)?;
    let database = dir.join("hub.db");
    let store = |e: ignite_store::StoreError| Error::Io(e.to_string());
    let conn = ignite_store::hub::open(&database).map_err(store)?;
    ignite_hub::routes::dispatch::set_hub_version(&conn, version).map_err(store)?;
    let running = ignite_hub::server::HubServer {
        config,
        listen: listen.to_owned(),
        database,
    }
    .start()?;
    // Served until the process ends. The accept loop ends only when `tiny_http` has failed to
    // accept for good, so its end stops the daemon and the login service restarts it.
    crash::spawn_loop("ignite-hub-accept", move || running.wait());
    let upkeep = ignite_hub::upkeep::Upkeep::new(Some(dir.join("backups"))).map_err(store)?;
    crash::spawn_loop("ignite-hub-upkeep", move || upkeep.run(conn, &SystemClock));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tip_and_idle_loops_do_their_work_in_their_own_classes() {
        assert_eq!(loop_class("ignite-tip"), Some(Class::Tip));
        assert_eq!(loop_class("ignite-idle"), Some(Class::Idle));
        assert_eq!(loop_class("ignite-sender"), None);
    }
}
