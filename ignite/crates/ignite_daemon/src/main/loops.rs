//! The daemon's background threads, and the hub that the hub's machine serves in a process of
//! its own (`ignite hub`). Each loop reports a failed round on standard error and tries again, so an
//! unreachable hub or a busy tree never stops the daemon.

use std::cell::RefCell;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use rusqlite::Connection;

use ignite_core::config::Config;
use ignite_core::protocol::Class;
use ignite_core::protocol::hub::Heartbeat;
use ignite_core::time::SystemClock;
use ignite_core::traits::{Error, Result};

use super::DaemonOptions;
use super::selfupdate::{Busy, SelfUpdate};
use crate::checks::Checks;
use crate::claim::Claims;
use crate::crash;
use crate::dispatchd::Dispatcher;
use crate::hubclient::HubClient;
use crate::idle::{Idle, ServerJobs};
use crate::land::Landing;
use crate::land::env::MachineTrees;
use crate::loop_health::{LoopHealth, LoopTicker};
use crate::manifests::Reload;
use crate::sched::Scheduler;
use crate::server::{Context, Server};
use crate::supervise::SupervisedLauncher;
use crate::tip::{CheckTipWork, TipLoop, TipOutcome, TipRoundVerb};
use crate::trees::Trees;
use crate::update::Home;

/// How often the outbox is sent and the heartbeat beats. Transcript lines are sent sooner,
/// as soon as they are queued (see `spawn_sender`).
const SEND_EVERY: Duration = Duration::from_secs(10);
/// How often the stall check looks at held tickets.
const TICK_EVERY: Duration = Duration::from_secs(60);
/// How often idle work looks for cores nobody uses.
const IDLE_EVERY: Duration = Duration::from_secs(30);
/// How often the disk loop looks at free space and at trees `ignite.toml` no longer declares.
/// A round measures the trees only as often as `Idle::hold_disk` allows.
const DISK_EVERY: Duration = Duration::from_secs(60);
/// How often the loop that ends verb processes that ignored their stop looks for them.
const OVERDUE_EVERY: Duration = Duration::from_secs(10);
/// How often fleet metrics look for new session transcripts.
const METRICS_EVERY: Duration = Duration::from_secs(600);
/// The pause after a failed round of tip verification.
const TIP_RETRY: Duration = Duration::from_secs(10);
/// How often the main checkout is fast-forwarded and its check manifests read again.
const MAIN_EVERY: Duration = Duration::from_secs(30);
/// How often the hub reads `ignite.toml` again for its routing facts (see [`serve_hub_at`]).
const ROUTING_EVERY: Duration = Duration::from_secs(30);

/// How long each loop may go without finishing a round before `ignite doctor` calls it stuck.
/// Each is far longer than the loop's own period, so that a slow hub or a long job is not
/// reported, and only a loop that has stopped is. A round of tip verification may run for an
/// hour, because it runs the checks.
const SENDER_STALLS_AFTER: Duration = Duration::from_mins(10);
const TIP_STALLS_AFTER: Duration = Duration::from_hours(3);
const CLAIMS_STALL_AFTER: Duration = Duration::from_mins(10);
const IDLE_STALLS_AFTER: Duration = Duration::from_mins(10);
/// A measurement walked about 290 GB of build directories for over ten minutes on jarvis.
const DISK_STALLS_AFTER: Duration = Duration::from_hours(1);
const OVERDUE_STALLS_AFTER: Duration = Duration::from_mins(10);
const METRICS_STALL_AFTER: Duration = Duration::from_hours(1);
const MAIN_STALLS_AFTER: Duration = Duration::from_mins(10);
/// A round of the ticket mirror is one long poll of the feed or one page of a copy.
const MIRROR_STALLS_AFTER: Duration = Duration::from_mins(30);
/// The pause after a round of the ticket mirror that failed or found nothing, so a hub that
/// answers the long poll at once (it has no room for another waiter) is not asked in a tight
/// loop.
const MIRROR_PAUSE: Duration = Duration::from_secs(1);
const MIRROR_RETRY: Duration = Duration::from_secs(10);

/// What the loops share, built by [`super::Daemon::start`].
pub struct Parts {
    pub ctx: Context,
    pub client: Arc<HubClient>,
    pub trees: Trees,
    /// Reads the main checkout's manifests again after the main loop fast-forwards it.
    pub manifests: Arc<Reload>,
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
    /// The emergency pause, which each heartbeat's reply updates.
    pub emergencies: Arc<crate::emergency::Emergencies>,
    /// Reports session titles to the hub, both as a hook on each command and from the sender
    /// loop for the sessions holding trees.
    pub titles: Arc<crate::session_title::TitleHook>,
    /// The CLI pool, when `ignite.toml` gives any app a CLI program (see `super::cli`).
    pub cli: Option<Arc<super::cli::CliMode>>,
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
        self.spawn_tip(server.clone());
        self.spawn_main();
        self.spawn_claims();
        self.spawn_idle(server.clone());
        self.spawn_disk();
        self.spawn_metrics();
        self.spawn_mirror();
        self.spawn_overdue_stops(server.clone());
        if let Some(cli) = &self.cli {
            cli.spawn(self);
        }
    }

    /// Ends a verb process that was asked to stop, by any path, and still runs a minute later
    /// (see [`crate::stop_overdue`]).
    fn spawn_overdue_stops(&self, server: Arc<Server>) {
        self.start_loop(
            "ignite-overdue-stops",
            OVERDUE_STALLS_AFTER,
            move |ticker| {
                loop {
                    thread::sleep(OVERDUE_EVERY);
                    ticker.round(|| {
                        server.end_overdue_stops();
                        Ok(())
                    });
                }
            },
        );
    }

    /// Sends the outbox, the heartbeat and any landing the hub has not heard of. After each
    /// heartbeat it compares the daemon's version with the hub's and restarts into a new
    /// executable once one is ready and no landing holds it back (see
    /// [`SelfUpdate::after_heartbeat`]).
    /// A release build runs on a thread of its own, so a round never waits for Cargo; the round
    /// only looks whether the build has finished and names it in the loop's step.
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
        let emergencies = self.emergencies.clone();
        let titles = self.titles.clone();
        let hostname = self.hostname.clone();
        self.start_loop("ignite-sender", SENDER_STALLS_AFTER, move |ticker| {
            // The hub's version from the last heartbeat it answered.
            let hub_version: RefCell<Option<String>> = RefCell::new(None);
            let beat = || {
                // A daemon that took on a release without restarting reports that release.
                let version = update
                    .version()
                    .map_or_else(|| version.clone(), |commit| commit.to_string());
                let reply = client.heartbeat(&Heartbeat {
                    machine: ctx.machine.clone(),
                    version,
                    workers: dispatcher.view().unwrap_or_default(),
                    families: current_families(&ctx.repo, &hostname),
                })?;
                *hub_version.borrow_mut() = Some(reply.hub_version);
                dispatcher
                    .pool()
                    .set_paused_everywhere(reply.paused_everywhere);
                emergencies.heard(reply.emergency);
                Ok(())
            };
            loop {
                ticker.round(|| {
                    let sent = crate::outbox::tick(&db, ctx.hub.as_ref(), &beat);
                    titles.report_holders(&ctx, &db);
                    landing.deliver_pending(&ctx);
                    if let Err(error) = update.poll_release() {
                        eprintln!("ignite: the release branch was not checked: {error}");
                    }
                    update.after_heartbeat(hub_version.borrow().as_deref(), busy(server.steps()));
                    reporter.tick(&ctx, sent.is_ok());
                    sent
                });
                // Between rounds, transcript lines go out as soon as they are queued, so someone
                // following a CLI worker on the dashboard sees them within about a second. Such
                // a send flushes the whole outbox, which only sends other entries sooner; the
                // heartbeat and the rest of the round keep their period.
                update.nap(SEND_EVERY, |most| {
                    if crate::outbox::wait_for_transcripts(most)
                        && let Err(error) = crate::outbox::flush(&db, ctx.hub.as_ref())
                    {
                        crate::hubclient::HUB_WRITES.failed("outbox not sent", &error);
                    }
                });
            }
        });
    }

    /// Starts the loop `name` on a thread of that name. It registers with the loop health,
    /// which `body` reports each round to, and the daemon exits if it ever stops (see
    /// [`crash::spawn_loop`]).
    pub(super) fn start_loop(
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

    /// One round of tip verification, as the daemon runs it on a thread of its own: over the
    /// machine's trees, keeping the verifier's memory in the local database, so a restarted
    /// daemon goes on from what the last round reported and held off.
    fn round_verb(&self) -> TipRoundVerb {
        let work = CheckTipWork {
            checks: self.checks.clone(),
            trees: Arc::new(MachineTrees {
                trees: self.trees.clone(),
            }),
            landing_trees: self.trees.landing_count(),
        };
        TipRoundVerb::new(
            Arc::new(work),
            self.version.clone(),
            self.hostname.clone(),
            Some(self.home.root.join("daemon.db")),
        )
    }

    /// See [`super::Daemon::verify_tip`].
    pub fn verify_tip(&self) -> Result<TipOutcome> {
        let tip = self.ctx.git.tip(&self.ctx.config.repository.staging)?;
        self.round_verb().verifier().verify(&self.ctx, &tip)
    }

    /// Verifies each new tip of the integration branch, each round in a verb process of its own
    /// that outlives a restart of the daemon (see [`TipLoop`]). A round that moved `master` has
    /// the new release built.
    fn spawn_tip(&self, server: Arc<Server>) {
        let mut tips = TipLoop::new(server, Arc::new(self.round_verb()));
        let (ctx, wait_s) = (self.ctx.clone(), self.feed_wait_s);
        let update = self.update.clone();
        self.start_loop("ignite-tip", TIP_STALLS_AFTER, move |ticker| {
            loop {
                let round = ticker.round(|| match tips.turn(&ctx, wait_s) {
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
                });
                if round.is_none() {
                    thread::sleep(TIP_RETRY);
                }
            }
        });
    }

    /// Keeps the main checkout's branches fast-forwarded and reads its check manifests again
    /// (see `crate::manifests`). This ran after each tip round until 2026-11-20, when a round
    /// that took over an hour left the checkout's `ignite.toml`, which the hub reads, an hour
    /// behind `staging`.
    fn spawn_main(&self) {
        let (trees, manifests) = (self.trees.clone(), self.manifests.clone());
        self.start_loop("ignite-main", MAIN_STALLS_AFTER, move |ticker| {
            loop {
                ticker.round(|| {
                    trees
                        .fast_forward_main()
                        .map_err(|error| format!("main checkout not fast-forwarded: {error}"))?;
                    match manifests.run() {
                        Ok(true) => eprintln!("ignite: read the changed check manifests"),
                        Ok(false) => {}
                        Err(error) => {
                            return Err(format!("check manifests not read again: {error}"));
                        }
                    }
                    Ok(())
                });
                thread::sleep(MAIN_EVERY);
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
    fn spawn_idle(&self, server: Arc<Server>) {
        let jobs = ServerJobs {
            server,
            repo: self.ctx.repo.clone(),
        };
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
                    // A job a round started runs in a verb process (`idle-job`) and is not
                    // waited for; until it finishes, the round does no other idle work. A
                    // restarted daemon finds the job's handle and settles it the same way.
                    match idle.settle(&jobs) {
                        Ok(true) => return Ok(()),
                        Ok(false) => {}
                        Err(error) => {
                            eprintln!("ignite: idle work: {error}");
                            return Err(format!("idle work: {error}"));
                        }
                    }
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
                    if let Err(error) = idle.drive(&ctx, &jobs) {
                        eprintln!("ignite: idle work: {error}");
                        failure = Some(format!("idle work: {error}"));
                    }
                    failure.map_or(Ok(()), Err)
                });
            }
        });
    }

    /// Holds the trees under their disk limit and the disk above its floor of free space, and
    /// removes the free trees that `ignite.toml` no longer declares. This is a loop of its own
    /// because a round of the idle loop waits for its job, which can wait hours for cores, and
    /// the machine is the most likely to fill its disk while it is busy.
    fn spawn_disk(&self) {
        let (idle, trees) = (self.idle.clone(), self.trees.clone());
        self.start_loop("ignite-disk", DISK_STALLS_AFTER, move |ticker| {
            loop {
                ticker.round(|| {
                    let mut failure = None;
                    crate::steps::step("removing trees ignite.toml no longer declares");
                    match trees.retire_undeclared() {
                        Ok(retired) if !retired.is_empty() => {
                            let names: Vec<String> =
                                retired.iter().map(ToString::to_string).collect();
                            eprintln!(
                                "ignite: removed trees ignite.toml no longer declares: {}",
                                names.join(", ")
                            );
                        }
                        Ok(_) => {}
                        Err(error) => {
                            eprintln!("ignite: undeclared trees not removed: {error}");
                            failure = Some(format!("undeclared trees not removed: {error}"));
                        }
                    }
                    if let Err(error) = idle.hold_disk() {
                        eprintln!("ignite: trees not held under their disk limit: {error}");
                        failure = Some(format!("trees not held under their disk limit: {error}"));
                    }
                    failure.map_or(Ok(()), Err)
                });
                thread::sleep(DISK_EVERY);
            }
        });
    }

    /// Keeps this machine's append-only copy of the hub's tickets (see [`crate::mirror`]). A
    /// mirror that cannot be opened is tried again each round, since the loop ending would stop
    /// the daemon.
    fn spawn_mirror(&self) {
        let (ctx, path, wait_s) = (
            self.ctx.clone(),
            self.home.root.join(crate::mirror::FILE),
            self.feed_wait_s,
        );
        self.start_loop("ignite-mirror", MIRROR_STALLS_AFTER, move |ticker| {
            let mut mirror: Option<crate::mirror::Mirror> = None;
            loop {
                let round = ticker.round(|| {
                    if mirror.is_none() {
                        mirror = Some(
                            crate::mirror::Mirror::open(&path, ctx.clock.clone())
                                .map_err(|error| format!("ticket mirror not opened: {error}"))?,
                        );
                    }
                    let Some(open) = mirror.as_mut() else {
                        unreachable!("opened above")
                    };
                    open.round(ctx.hub.as_ref(), wait_s).map_err(|error| {
                        eprintln!("ignite: ticket mirror: {error}");
                        format!("ticket mirror: {error}")
                    })
                });
                match round {
                    Some(progress) if progress.copying => {}
                    Some(_) => thread::sleep(MIRROR_PAUSE),
                    None => thread::sleep(MIRROR_RETRY),
                }
            }
        });
    }

    fn spawn_metrics(&self) {
        let (ctx, state) = (self.ctx.clone(), self.home.root.join("metrics.json"));
        self.start_loop("ignite-metrics", METRICS_STALL_AFTER, move |ticker| {
            // Rounds stay a period apart across restarts, instead of one at every start.
            thread::sleep(crate::metrics::first_round_in(
                &state,
                ctx.clock.now(),
                METRICS_EVERY,
            ));
            loop {
                ticker.round(|| {
                    crate::metrics::post_new_rows(&ctx, &state).map_err(|error| {
                        crate::hubclient::HUB_WRITES.failed("metrics not posted", &error);
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

/// Where the hub listens: the writable listener (`IGNITE_HUB_LISTEN`) and, if set, the read-only
/// one that the public address proxies (`IGNITE_HUB_PUBLIC_LISTEN`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HubAddresses {
    pub listen: String,
    pub public: Option<String>,
}

impl HubAddresses {
    /// The addresses `lookup` gives for [`ignite_os::service::HUB_VARIABLES`], or `None` when the
    /// writable listener is not set.
    pub fn from(lookup: impl Fn(&str) -> Option<String>) -> Option<HubAddresses> {
        Some(HubAddresses {
            listen: lookup("IGNITE_HUB_LISTEN").filter(|v| !v.is_empty())?,
            public: lookup("IGNITE_HUB_PUBLIC_LISTEN").filter(|v| !v.is_empty()),
        })
    }

    /// The addresses in this process's environment.
    pub fn from_env() -> Option<HubAddresses> {
        HubAddresses::from(|name| std::env::var(name).ok())
    }
}

/// What runs that a restart waits for: a verb in a part that may not be cut short, and any
/// landing, whether it runs on the daemon's threads or in a verb process of its own.
fn busy(steps: &crate::steps::Steps) -> Busy {
    let landing = steps.live().iter().any(
        |activity| matches!(activity.kind(), crate::steps::Kind::Verb(verb) if verb == "land"),
    );
    Busy {
        critical: steps.critical_now(),
        landing,
    }
}

/// Whether the daemon serves the hub itself. The hub has a login service of its own
/// (`ignite hub`, installed by `ignite install`), so a daemon restart does not take it away.
/// A daemon still started with `IGNITE_HUB_LISTEN` on a machine without that service is one
/// installed before the hub had a service, which a release reached by restarting: it keeps
/// serving the hub until `ignite install` moves the hub to its own service, so that the
/// upgrade does not leave every machine without a hub.
pub fn daemon_serves_hub(addresses: Option<&HubAddresses>, hub_service: bool) -> bool {
    addresses.is_some() && !hub_service
}

/// On a hub's machine that has no hub service yet (see [`daemon_serves_hub`]), serves the hub
/// and runs its upkeep for the life of the daemon.
pub fn serve_hub_if_asked(options: &DaemonOptions, hub_service: bool) -> Result<()> {
    let addresses = HubAddresses::from_env();
    if !daemon_serves_hub(addresses.as_ref(), hub_service) {
        if addresses.is_some() {
            eprintln!(
                "ignite: not serving the hub: its own login service, `ignite hub`, serves it"
            );
        }
        return Ok(());
    }
    let Some(addresses) = addresses else {
        return Ok(());
    };
    eprintln!(
        "ignite: serving the hub inside the daemon, since this machine has no hub service; \
         run `ignite install` to give the hub a login service of its own"
    );
    serve_hub_at(
        &options.repo,
        &addresses,
        &options.home.root.join("hub"),
        &options.version,
        std::time::Duration::ZERO,
    )
}

/// Serves the hub's database in `dir` at both of `addresses` and starts its upkeep thread, for
/// the life of the process. A listener whose address is still taken is tried again for up to
/// `bind_for`: a hub restarted by its login service may start before the old process has let go
/// of the port.
pub fn serve_hub_at(
    repo: &Path,
    addresses: &HubAddresses,
    dir: &Path,
    version: &str,
    bind_for: Duration,
) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let config = super::read_config(repo)?;
    let database = dir.join("hub.db");
    let store = |e: ignite_store::StoreError| Error::Io(e.to_string());
    let conn = ignite_store::hub::open(&database).map_err(store)?;
    ignite_hub::routes::dispatch::set_hub_version(&conn, version).map_err(store)?;
    let wordmark = read_wordmark(repo, &config);
    let server = |listen: &str| ignite_hub::server::HubServer {
        config: config.clone(),
        listen: listen.to_owned(),
        database: database.clone(),
        wordmark: wordmark.clone(),
    };
    let running = retry_bind(bind_for, || server(&addresses.listen).start())?;
    // Served until the process ends. The accept loop ends only when `tiny_http` has failed to
    // accept for good, so its end stops the process and the login service restarts it.
    crash::spawn_loop("ignite-hub-accept", move || running.wait());
    // The read-only listener (`IGNITE_HUB_PUBLIC_LISTEN`) is the address that `tailscale
    // funnel` publishes. It answers only `GET` and `HEAD` and never serves `/export`, so the
    // public internet can read the dashboard but cannot write.
    if let Some(public) = &addresses.public {
        let running = retry_bind(bind_for, || server(public).start_read_only())?;
        crash::spawn_loop("ignite-hub-public-accept", move || running.wait());
    }
    let upkeep = ignite_hub::upkeep::Upkeep::new(Some(dir.join("backups"))).map_err(store)?;
    crash::spawn_loop("ignite-hub-upkeep", move || upkeep.run(conn, &SystemClock));
    let routing_conn = ignite_store::hub::open(&database).map_err(store)?;
    let repo = repo.to_path_buf();
    crash::spawn_loop("ignite-hub-routing", move || {
        loop {
            thread::sleep(ROUTING_EVERY);
            if let Err(error) = reload_routing(&repo, &routing_conn) {
                eprintln!("ignite hub: routing not read again: {error}");
            }
        }
    });
    Ok(())
}

/// Records the routing facts of the repository's `ignite.toml` (`ignite_core::owner::Routing`)
/// when they differ from the ones recorded, and says whether it did. The hub records them at
/// start, but `ignite.toml` lies outside Ignite's source, so a change to it, such as a new
/// default owner, builds no release and restarts nothing: without this, dispatch would keep
/// routing by the old file until some later restart.
fn reload_routing(repo: &Path, conn: &Connection) -> Result<bool> {
    let store = |e: ignite_store::StoreError| Error::Io(e.to_string());
    let routing = ignite_core::owner::Routing::from_config(&super::read_config(repo)?);
    if ignite_store::hub::owners::routing(conn).map_err(store)? == routing {
        return Ok(false);
    }
    ignite_store::hub::owners::record_routing(conn, &routing).map_err(store)?;
    eprintln!("ignite hub: recorded the changed routing of ignite.toml");
    Ok(true)
}

/// The file `[hub] wordmark` names, read from the repository. A file that cannot be read is
/// logged and the dashboard shows the name "Ignite" instead, since a missing picture should not
/// stop the hub.
fn read_wordmark(repo: &Path, config: &Config) -> Option<String> {
    let path = repo.join(config.hub.wordmark.as_ref()?);
    match std::fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(e) => {
            eprintln!(
                "ignite hub: cannot read the wordmark {}: {e}",
                path.display()
            );
            None
        }
    }
}

/// Calls `start` until it succeeds or `window` has passed, pausing briefly between tries.
fn retry_bind<T>(window: Duration, start: impl Fn() -> std::io::Result<T>) -> Result<T> {
    let began = std::time::Instant::now();
    loop {
        match start() {
            Ok(running) => return Ok(running),
            Err(error) if began.elapsed() >= window => return Err(Error::Io(error.to_string())),
            Err(_) => thread::sleep(Duration::from_millis(100)),
        }
    }
}

/// The families the main checkout's `ignite.toml` lists for this machine now, empty for every
/// family, for the heartbeat to report: the hub otherwise holds the list the machine registered
/// with, however its `ignite.toml` changed since. `None`, which leaves the hub's list, when the
/// file cannot be read or parsed (such as halfway through a merge) or no longer declares the
/// machine.
fn current_families(repo: &std::path::Path, hostname: &str) -> Option<Vec<String>> {
    crate::manifests::machine_families(repo, hostname)
        .ok()
        .flatten()
        .map(Option::unwrap_or_default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_daemon_serves_the_hub_only_where_no_hub_service_does() {
        let addresses = HubAddresses::from(|name| {
            (name == "IGNITE_HUB_LISTEN").then(|| "127.0.0.1:7345".to_owned())
        });
        assert_eq!(
            addresses,
            Some(HubAddresses {
                listen: "127.0.0.1:7345".to_owned(),
                public: None
            })
        );
        assert!(daemon_serves_hub(addresses.as_ref(), false));
        assert!(!daemon_serves_hub(addresses.as_ref(), true));
        assert!(!daemon_serves_hub(None, false));
        assert_eq!(HubAddresses::from(|_| Some(String::new())), None);
    }

    #[test]
    fn a_listener_whose_port_frees_up_is_served_within_the_window() {
        let tries = std::cell::Cell::new(0);
        let started = retry_bind(Duration::from_secs(5), || {
            tries.set(tries.get() + 1);
            if tries.get() < 3 {
                Err(std::io::Error::from(std::io::ErrorKind::AddrInUse))
            } else {
                Ok(())
            }
        });
        assert!(started.is_ok());
        assert_eq!(tries.get(), 3);
        let never = retry_bind(Duration::ZERO, || {
            Err::<(), _>(std::io::Error::from(std::io::ErrorKind::AddrInUse))
        });
        assert!(never.is_err());
    }

    #[test]
    fn a_changed_ignite_toml_reaches_the_hubs_routing() {
        let dir = ignite_core::test_files::temp_dir()
            .join(format!("ignite-routing-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let conn = ignite_store::hub::open_in_memory().unwrap();
        let config = |owner: &str| {
            format!(
                "[repository]\nremote = \"origin\"\nstaging = \"s\"\nmaster = \"m\"\n\
                 trees = \"t\"\nchecks = \"c\"\n[hub]\nurl = \"http://127.0.0.1:0\"\n\
                 project = \"p\"\n[gate]\nthreshold = 0.004\ncap = 400\n[pool]\n\
                 default_owner = \"{owner}\"\n[machine.jarvis]\nos = \"macos\"\n\
                 ticket_trees = 1\nlanding_trees = 1\n"
            )
        };
        let toml = dir.join("ignite.toml");
        std::fs::write(&toml, config("jarvis/claude")).unwrap();
        assert!(reload_routing(&dir, &conn).unwrap());
        assert!(!reload_routing(&dir, &conn).unwrap());
        std::fs::write(&toml, config("jarvis/codex")).unwrap();
        assert!(reload_routing(&dir, &conn).unwrap());
        let routing = ignite_store::hub::owners::routing(&conn).unwrap();
        assert_eq!(
            routing.default_owner,
            Some(ignite_core::owner::Owner::new("jarvis", Some("codex")))
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_tip_and_idle_loops_do_their_work_in_their_own_classes() {
        assert_eq!(loop_class("ignite-tip"), Some(Class::Tip));
        assert_eq!(loop_class("ignite-idle"), Some(Class::Idle));
        assert_eq!(loop_class("ignite-sender"), None);
    }
}
