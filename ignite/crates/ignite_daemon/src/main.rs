//! The daemon's entry point: reads `ignite.toml` and the check manifests, builds the real
//! implementation of every part (the scheduler, the supervisor, build trees, builds, the check
//! runner and its cache, landing, tip verification, idle work, claims and the dispatcher),
//! registers every verb with one [`Server`] and serves the machine's socket.
//!
//! [`Daemon::start`] does the assembly from [`DaemonOptions`], so a test can run two daemons in
//! one process, each with its own home folder and machine. [`run_daemon`], the hidden
//! `ignite daemon` subcommand, takes the machine's lock and fills the options from the
//! environment.
//!
//! The hub's machine runs the hub as a login service of its own, [`run_hub`] (`ignite hub`):
//! when `IGNITE_HUB_LISTEN` names an address, such as `127.0.0.1:7345`, it serves the hub's
//! database `hub/hub.db` in the home folder there, and `IGNITE_HUB_PUBLIC_LISTEN` a read-only
//! listener, and runs the hub's upkeep, writing backups into `hub/backups`. A daemon serves the
//! hub itself only on a machine whose hub has no service yet (see
//! [`loops::daemon_serves_hub`]).

pub(crate) mod assemble;
mod desk;
pub mod drill;
mod loops;
mod restart_window;
mod selfupdate;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ignite_core::config::Config;
use ignite_core::ids::Commit;
use ignite_core::manifest::Manifest;
use ignite_core::traits::{Error, Result};

use crate::doctor::NativeHost;
use crate::loop_health::LoopHealth;
use crate::server::{Context, Server};
use crate::update::{ExecRestart, Home, ReleaseBuilder, Startup, TreeBuilder};

/// What one daemon is started with.
#[derive(Clone, Debug)]
pub struct DaemonOptions {
    /// The repository's main checkout.
    pub repo: PathBuf,
    /// The machine's Ignite folder: its identity, local database, runs and records.
    pub home: Home,
    /// The machine's name, which names its table in `ignite.toml`.
    pub hostname: String,
    /// The `ignite` executable that supervisors and the compiler wrapper scripts run.
    pub ignite: PathBuf,
    /// The Ignite commit this daemon runs, which records name.
    pub version: String,
    /// The longest wait of one long poll of the hub's feed, in seconds.
    pub feed_wait_s: u32,
}

/// One assembled daemon.
pub struct Daemon {
    pub server: Arc<Server>,
    pub ctx: Context,
    pub options: DaemonOptions,
    parts: loops::Parts,
}

/// When [`Daemon::start_with`] creates the trees that are missing and resets the ones a stopped
/// daemon left in the middle of something.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeSetup {
    /// Before it returns, so every tree is ready when the daemon is.
    Now,
    /// On a thread of its own, after marking them `resetting` (see `crate::trees`), so a
    /// restarted daemon answers without waiting for `git`.
    Background,
}

impl Daemon {
    /// Builds every part, registers the machine with the hub if it has no token yet and creates
    /// the build trees. The background loops start with [`Daemon::run_loops`], and serving is
    /// left to the caller.
    pub fn start(options: DaemonOptions) -> Result<Daemon> {
        Daemon::start_with(options, TreeSetup::Now)
    }

    /// [`Daemon::start`], with the trees set up as `setup` says.
    pub fn start_with(options: DaemonOptions, setup: TreeSetup) -> Result<Daemon> {
        let base = assemble::Base::read(options)?;
        let machinery = assemble::machinery(&base, setup)?;
        let (checks, landing, idle) = assemble::checking(
            &base,
            &machinery.trees,
            Arc::new(crate::land::express::SchedulerLender(
                machinery.scheduler.clone(),
            )),
        )?;
        let ctx = Context {
            config: base.config.clone(),
            repo: base.options.repo.clone(),
            machine: base.machine.clone(),
            clock: base.clock.clone(),
            hub: base.hub.clone(),
            git: base.git.clone(),
            launcher: machinery.launcher.clone(),
            builds: machinery.builds.clone(),
            session: None,
            tree: None,
        };
        let desks = assemble::desks(&base, &ctx, &machinery)?;
        let health = LoopHealth::new(base.clock.clone());
        let parts = loops::Parts {
            ctx: ctx.clone(),
            client: base.client.clone(),
            trees: machinery.trees.clone(),
            manifests: Arc::new(crate::manifests::Reload {
                repo: base.options.repo.clone(),
                config: base.config.clone(),
                manifest: base.live.clone(),
                trees: machinery.trees.clone(),
            }),
            checks,
            landing,
            idle,
            claims: desks.claims.clone(),
            dispatcher: desks.dispatcher.clone(),
            scheduler: machinery.scheduler.clone(),
            sender_db: Mutex::new(Some(base.open()?)),
            home: base.options.home.clone(),
            version: base.options.version.clone(),
            hostname: base.options.hostname.clone(),
            feed_wait_s: base.options.feed_wait_s,
            update: Arc::new(self_update(
                &base,
                &machinery.launcher,
                &machinery.scheduler,
            )),
            health: health.clone(),
            reporter: Arc::new(crate::justiciar::Reporter::new(
                base.options.home.root.clone(),
                crate::doctor::Host::hostname(&crate::doctor::NativeHost),
                base.options.version.clone(),
                health,
                desks.dispatcher.clone(),
            )),
            emergencies: Arc::new(crate::emergency::Emergencies::new(
                base.clock.clone(),
                Some(base.open()?),
            )),
            titles: Arc::new(crate::session_title::TitleHook::new(
                crate::session_title::app_data(&|name: &str| std::env::var(name).ok()),
            )),
        };
        let server = assemble::server(&base, &ctx, &parts, &desks)?;
        let _ = parts.update.drain.set(server.clone());
        server.register_late(Box::new(drill::RestartVerb {
            update: parts.update.clone(),
            server: Arc::downgrade(&server),
            executable: base.options.ignite.clone(),
            database: base.options.home.root.join("daemon.db"),
        }));
        Ok(Daemon {
            server,
            ctx,
            options: base.options,
            parts,
        })
    }

    /// Starts the background loops: the outbox and heartbeat, tip verification, idle work,
    /// the stall check, the main checkout's fast-forward and fleet metrics.
    pub fn run_loops(&self) {
        self.parts.spawn(&self.server);
    }

    /// What each background loop has done lately, which `status` and `doctor` show.
    pub fn loop_health(&self) -> LoopHealth {
        self.parts.health.clone()
    }

    /// Verifies the integration branch's newest commit once, now, as one round of the tip loop
    /// would.
    pub fn verify_tip(&self) -> Result<crate::tip::TipOutcome> {
        self.parts.verify_tip()
    }

    /// Serves the socket until the process ends.
    pub fn serve(&self, socket: &Path) -> std::io::Result<()> {
        crate::server::serve(&self.server, &socket.to_string_lossy())
    }
}

/// Updates this daemon to new releases: builds them in `update/update-tree` in the home
/// folder, apart from the build trees, and restarts with [`ExecRestart`]. The build and its
/// selftest run on the machine's express cores, borrowing free cores from `scheduler` (see
/// [`crate::update::direct`]), or as supervised commands through `launcher` on a machine with
/// no express cores.
fn self_update(
    base: &assemble::Base,
    launcher: &Arc<dyn ignite_core::traits::Launcher>,
    scheduler: &Arc<crate::sched::Scheduler>,
) -> selfupdate::SelfUpdate {
    let options = &base.options;
    let update = options.home.root.join("update");
    let express = |log: &str| -> Arc<dyn ignite_core::traits::Launcher> {
        let cores = base.declared.express_cores;
        if cores == 0 {
            return launcher.clone();
        }
        Arc::new(crate::update::direct::ExpressLauncher {
            cores,
            lender: Some(Arc::new(crate::land::express::SchedulerLender(
                scheduler.clone(),
            ))),
            log: update.join(log),
        })
    };
    selfupdate::SelfUpdate {
        own: Commit::new(&options.version).ok(),
        git: base.git.clone(),
        builder: Arc::new(ReleaseBuilder {
            tree: TreeBuilder {
                repo: options.repo.clone(),
                work: options.home.root.join("update"),
            },
            launcher: express("build.log"),
        }),
        store: options.home.store(),
        master: base.config.repository.release_branch().to_owned(),
        checks: crate::update::scope::checks_dir(&base.config),
        restart: Box::new(ExecRestart),
        ready: Mutex::new(None),
        rounds: std::sync::atomic::AtomicU32::new(0),
        ahead_of: Mutex::default(),
        candidate_seen: Mutex::default(),
        building: Mutex::default(),
        failed: Mutex::default(),
        selftest: Arc::new(crate::update::selftest::SupervisedSelftest {
            launcher: express("selftest.log"),
            repo: options.repo.clone(),
        }),
        hub: base.hub.clone(),
        clock: base.clock.clone(),
        machine: base.machine.clone(),
        host: options.hostname.clone(),
        reports: Mutex::default(),
        hub_service: Arc::new(NativeHubService),
        hub_kicked: Mutex::default(),
        drain: std::sync::OnceLock::new(),
        adopted: Mutex::default(),
        release_seen: Mutex::default(),
        window: Mutex::new(restart_window::RestartWindow::new(base.config.self_update)),
    }
}

/// This machine's hub login service, through `ignite_os::service`.
struct NativeHubService;

impl selfupdate::HubService for NativeHubService {
    fn installed(&self) -> bool {
        use ignite_os::service::Service;
        ignite_os::Native.hub_installed().unwrap_or(false)
    }

    fn kickstart(&self) -> std::io::Result<()> {
        use ignite_os::service::Service;
        ignite_os::Native.kickstart_hub()
    }
}

/// How long `ignite hub` tries again to listen on an address that is taken: its login service
/// may start it again before the old hub process has let go of the port.
const HUB_BIND_WINDOW: std::time::Duration = std::time::Duration::from_secs(10);

/// Raises the soft open-file limit before anything opens a file. A login service inherits
/// launchd's limit of 256 descriptors, which a burst of builds passes, so builds failed with
/// "Too many open files". The commands the process starts inherit the raised limit.
fn raise_open_file_limit() {
    if let Err(error) = ignite_os::files::raise_open_file_limit(ignite_os::files::WANTED_OPEN_FILES)
    {
        eprintln!("ignite: cannot raise the open-file limit: {error}");
    }
}

/// The hidden `ignite hub` subcommand, which the hub's login service runs from `bin/current`:
/// serves the hub at the addresses in `IGNITE_HUB_LISTEN` and `IGNITE_HUB_PUBLIC_LISTEN`, and
/// runs its upkeep, until the process ends. It does nothing else, so it starts in well under a
/// second, and a daemon restart or crash on this machine leaves it serving. The hub reports
/// this executable's commit as its version in heartbeat replies, which every daemon follows.
/// Returns the exit code.
pub fn run_hub(cwd: &Path) -> i32 {
    raise_open_file_limit();
    let home = Home::from_env();
    let Some(addresses) = loops::HubAddresses::from_env() else {
        eprintln!(
            "ignite: IGNITE_HUB_LISTEN is not set, so there is no address to serve the hub at"
        );
        return 1;
    };
    let repo = checkout(cwd, &home);
    let version = running_version();
    crate::crash::install_hook(home.root.clone(), version.clone());
    // A daemon asked to stop (`launchctl kickstart -k`, `bootout`, an install) records a clean
    // stop before it ends, so that neither crash-loop guard counts the restart.
    let stop_home = home.clone();
    if let Err(error) = ignite_os::terminate::on_terminate(Box::new(move || {
        crate::update::record_clean_stop(&stop_home, "asked to stop (SIGTERM)");
    })) {
        eprintln!("ignite: a stop will count as a crash: {error}");
    }
    let dir = home.root.join("hub");
    if let Err(error) = loops::serve_hub_at(&repo, &addresses, &dir, &version, HUB_BIND_WINDOW) {
        eprintln!("ignite: the hub did not start: {error}");
        return 1;
    }
    eprintln!(
        "ignite: serving the hub {version} at {}{}",
        addresses.listen,
        addresses
            .public
            .as_ref()
            .map_or_else(String::new, |public| format!(", read-only at {public}"))
    );
    // The listeners and the upkeep run on threads of their own; any of them stopping ends the
    // process (see `crate::crash::spawn_loop`).
    loop {
        std::thread::park();
    }
}

/// This executable, resolved, and the commit it was built from ("unknown" for one that is not
/// in the executable store).
fn running_executable() -> (PathBuf, String) {
    let ignite = std::env::current_exe()
        .and_then(ignite_os::paths::canonicalize)
        .unwrap_or_default();
    let version = crate::update::ExecutableStore::version_of(&ignite)
        .map_or_else(|| "unknown".to_owned(), |commit| commit.to_string());
    (ignite, version)
}

fn running_version() -> String {
    running_executable().1
}

/// The hidden `ignite daemon` subcommand: runs the daemon until the process ends, for the
/// main checkout of the repository at `cwd` or, outside one (the login service starts it in
/// `/`), for the checkout `ignite install` recorded. Returns the exit code.
pub fn run_daemon(cwd: &Path) -> i32 {
    raise_open_file_limit();
    let home = Home::from_env();
    // First, before anything this build could fail at: records the start, and goes back to the
    // last-good build if this one crash-loops.
    if let Some(code) = crate::update::guard_start(&home) {
        return code;
    }
    // A rollback drill: a build the fault file names exits as one that crashes at start would.
    if let Some(code) = drill::fault_exit(&home, running_version) {
        return code;
    }
    let repo = checkout(cwd, &home);
    // Held for the life of the process: one daemon per machine.
    let _lock = match crate::machine_lock::hold(&home) {
        Ok(lock) => lock,
        Err(error) => {
            crate::update::forget_start(&home);
            eprintln!("ignite: {error}");
            return 1;
        }
    };
    let (ignite, version) = running_executable();
    // A newer release must not delete this one while it runs.
    crate::update::hold_release(&ignite);
    // From here on, a panic anywhere in the daemon leaves a record in the home folder.
    crate::crash::install_hook(home.root.clone(), version.clone());
    // A daemon asked to stop (`launchctl kickstart -k`, `bootout`, an install) records a clean
    // stop before it ends, so that neither crash-loop guard counts the restart.
    let stop_home = home.clone();
    if let Err(error) = ignite_os::terminate::on_terminate(Box::new(move || {
        crate::update::record_clean_stop(&stop_home, "asked to stop (SIGTERM)");
    })) {
        eprintln!("ignite: a stop will count as a crash: {error}");
    }
    let store = home.store();
    let own = Commit::new(&version).ok();
    crate::doctor::remember_running_version(own.clone());
    if let Some(own) = &own
        && let Startup::RollBack { to, reason } = store.startup(own, crate::update::started_at())
    {
        eprintln!("ignite: rolling back: {reason}");
        let error = crate::update::restart_into(&ExecRestart, &store.path(&to));
        eprintln!("ignite: {error}");
        return 1;
    }
    if let Some(own) = own {
        // A release that is still up after a while has come up: its trial ends, and later it
        // becomes the last-good build.
        crate::update::spawn_health_marks(store, own);
    }
    let options = DaemonOptions {
        repo,
        home: home.clone(),
        hostname: crate::doctor::Host::hostname(&NativeHost),
        ignite,
        version,
        feed_wait_s: 60,
    };
    // The socket is served before the rest of the start: a request that arrives meanwhile
    // waits for the daemon to be ready instead of being refused (see `crate::server::Starting`).
    let socket = crate::server::default_socket();
    let listener = match listen(&socket) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("ignite: cannot serve the socket: {error}");
            return 1;
        }
    };
    let starting = crate::server::Starting::new();
    let accept = {
        let starting = starting.clone();
        std::thread::Builder::new()
            .name("ignite-accept".to_owned())
            .spawn(move || crate::server::serve_starting(&starting, listener.as_ref()))
    };
    let accept = match accept {
        Ok(accept) => accept,
        Err(error) => {
            eprintln!("ignite: cannot start the thread that serves the socket: {error}");
            return 1;
        }
    };
    let hub_service = {
        use ignite_os::service::Service;
        ignite_os::Native.hub_installed().unwrap_or(false)
    };
    // A hub the daemon cannot serve, such as one whose address the hub's own service already
    // holds, does not stop the daemon: the daemon is every verb on this machine, and the hub
    // has its own service.
    if let Err(error) = loops::serve_hub_if_asked(&options, hub_service) {
        eprintln!("ignite: the daemon does not serve the hub: {error}");
    }
    let started = Daemon::start_with(options, TreeSetup::Background)
        .map_err(|error| format!("the daemon did not start: {error}"));
    let daemon = match started {
        Ok(daemon) => daemon,
        Err(reason) => {
            eprintln!("ignite: {reason}");
            starting.failed(reason);
            // Gives the requests that waited for the start a moment to read why it failed.
            std::thread::sleep(std::time::Duration::from_millis(500));
            return 1;
        }
    };
    starting.ready(daemon.server.clone());
    daemon.run_loops();
    match accept.join() {
        Ok(Ok(())) => 0,
        Ok(Err(error)) => {
            eprintln!("ignite: cannot serve the socket: {error}");
            1
        }
        Err(_) => 1,
    }
}

/// Listens on the daemon's socket. With the machine's lock held, a listener already on it can
/// only be a daemon that is exiting (on Windows a restart starts the new process before the old
/// one's pipe is gone), so a socket in use is tried again for as long as the lock was waited for.
fn listen(socket: &Path) -> std::io::Result<Box<dyn ignite_os::ipc::Listener>> {
    use ignite_os::ipc::Ipc;
    let name = socket.to_string_lossy();
    let started = std::time::Instant::now();
    loop {
        match ignite_os::Native.listen(&name) {
            Err(error)
                if error.kind() == std::io::ErrorKind::AddrInUse
                    && started.elapsed() < crate::machine_lock::RESTART_WAIT =>
            {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            other => return other,
        }
    }
}

/// The main checkout of the repository at `cwd` if `cwd` holds an `ignite.toml`, else the
/// checkout recorded in the home folder. A daemon started in a linked worktree serves the main
/// checkout, as one started by the login service does.
pub(crate) fn checkout(cwd: &Path, home: &Home) -> PathBuf {
    if cwd.join("ignite.toml").is_file() {
        return crate::update::main_checkout(cwd);
    }
    std::fs::read_to_string(home.checkout_file())
        .map_or_else(|_| cwd.to_path_buf(), |text| PathBuf::from(text.trim()))
}

pub(crate) fn read_config(repo: &Path) -> Result<Config> {
    let path = repo.join("ignite.toml");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| Error::Invalid(format!("{}: {e}", path.display())))?;
    let (config, unknown) = Config::parse_with_unknown(&path.to_string_lossy(), &text)
        .map_err(|e| Error::Invalid(e.to_string()))?;
    for key in unknown {
        eprintln!(
            "ignite: {}: ignoring the unknown key {key}; a newer Ignite may know it",
            path.display()
        );
    }
    Ok(config)
}

/// Every `*.toml` in the repository's manifest directory, parsed together.
pub(crate) fn read_manifest(repo: &Path, config: &Config) -> std::result::Result<Manifest, String> {
    crate::manifests::read_directory(&repo.join(&config.repository.checks))
}
