//! The daemon's background threads, and the hub that the hub's machine serves beside its
//! daemon. Each loop reports a failed round on standard error and tries again, so an
//! unreachable hub or a busy tree never stops the daemon.

use std::cell::RefCell;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use rusqlite::Connection;

use ignite_core::protocol::hub::Heartbeat;
use ignite_core::time::SystemClock;
use ignite_core::traits::{Error, Result};

use super::DaemonOptions;
use super::selfupdate::SelfUpdate;
use crate::checks::Checks;
use crate::claim::Claims;
use crate::dispatchd::Dispatcher;
use crate::hubclient::HubClient;
use crate::idle::Idle;
use crate::land::Landing;
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
    pub feed_wait_s: u32,
    pub update: Arc<SelfUpdate>,
}

impl Parts {
    /// Starts every loop. A second call starts nothing. `server` reports which verbs are
    /// still running, which an update waits for.
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
    /// heartbeat it compares the daemon's version with the hub's: it stops asking for work
    /// while an update waits for running commands, and restarts into the new executable once
    /// none runs.
    fn spawn_sender(&self, db: Connection, server: Arc<Server>) {
        let (ctx, client, landing, dispatcher, version) = (
            self.ctx.clone(),
            self.client.clone(),
            self.landing.clone(),
            self.dispatcher.clone(),
            self.version.clone(),
        );
        let (scheduler, update) = (self.scheduler.clone(), self.update.clone());
        thread::spawn(move || {
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
                crate::outbox::tick(&db, ctx.hub.as_ref(), &beat);
                landing.deliver_pending(ctx.hub.as_ref());
                let running = running_commands(&server, &scheduler);
                let may_ask = update.after_heartbeat(hub_version.borrow().as_deref(), running);
                dispatcher.set_draining(!may_ask);
                thread::sleep(SEND_EVERY);
            }
        });
    }

    /// Verifies each new tip of the integration branch, then keeps the main checkout's
    /// branches fast-forwarded.
    fn verifier(&self) -> TipVerifier {
        let work = CheckTipWork {
            checks: self.checks.clone(),
            trees: Arc::new(self.trees.clone()),
        };
        TipVerifier::new(Arc::new(work), self.version.clone())
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
        thread::spawn(move || {
            loop {
                match verifier.round(&ctx, wait_s) {
                    Ok(Some((_, TipOutcome::Verified))) => {
                        if let Err(error) = update.after_master_moved() {
                            eprintln!("ignite: the new release was not built: {error}");
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        eprintln!("ignite: tip verification: {error}");
                        thread::sleep(TIP_RETRY);
                    }
                }
                if let Err(error) = trees.fast_forward_main() {
                    eprintln!("ignite: main checkout not fast-forwarded: {error}");
                }
            }
        });
    }

    fn spawn_claims(&self) {
        let (ctx, claims) = (self.ctx.clone(), self.claims.clone());
        thread::spawn(move || {
            loop {
                thread::sleep(TICK_EVERY);
                claims.tick(&ctx);
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
        thread::spawn(move || {
            loop {
                thread::sleep(IDLE_EVERY);
                let status = scheduler.status();
                if status.granted > 0 || status.outside * 2 >= status.cores {
                    continue;
                }
                if let Ok(tip) = ctx.git.tip(&ctx.config.repository.staging)
                    && let Err(error) = trees.keep_one_built(&tip)
                {
                    eprintln!("ignite: no tree kept built: {error}");
                }
                if let Some(job) = idle.next_job(&ctx)
                    && let Err(error) = idle.run(&ctx, &job)
                {
                    eprintln!("ignite: idle work {job:?}: {error}");
                }
            }
        });
    }

    fn spawn_metrics(&self) {
        let (ctx, state) = (self.ctx.clone(), self.home.root.join("metrics.json"));
        thread::spawn(move || {
            loop {
                if let Err(error) = crate::metrics::post_new_rows(&ctx, &state) {
                    eprintln!("ignite: metrics not posted: {error}");
                }
                thread::sleep(METRICS_EVERY);
            }
        });
    }
}

/// The commands an update waits for: verbs still running in the server, and commands that
/// hold or wait for cores, such as the checks of tip verification and idle work.
fn running_commands(server: &Server, scheduler: &Scheduler) -> usize {
    let held: u32 = scheduler
        .status()
        .classes
        .iter()
        .map(|class| class.running + class.waiting)
        .sum();
    server.running_verbs() + held as usize
}

/// Collects the runs a stopped daemon left, each on a thread that waits for its supervisor.
pub fn collect_unfinished(launcher: &SupervisedLauncher) {
    for run in launcher.unfinished() {
        thread::spawn(move || {
            run.wait();
        });
    }
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
    // Served until the process ends.
    std::mem::forget(running);
    let upkeep = ignite_hub::upkeep::Upkeep::new(Some(dir.join("backups"))).map_err(store)?;
    thread::spawn(move || upkeep.run(conn, &SystemClock));
    Ok(())
}
