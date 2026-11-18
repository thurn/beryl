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
//! The daemon on the hub's machine also serves the hub: when `IGNITE_HUB_LISTEN` names an
//! address, such as `127.0.0.1:7345`, it serves the hub's database `hub/hub.db` in the home
//! folder there and runs the hub's upkeep, writing backups into `hub/backups`.

mod assemble;
mod desk;
mod loops;
mod selfupdate;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ignite_core::config::Config;
use ignite_core::ids::Commit;
use ignite_core::manifest::Manifest;
use ignite_core::traits::{Clock, Error, Result};

use crate::doctor::NativeHost;
use crate::loop_health::LoopHealth;
use crate::server::{Context, Server};
use crate::update::{ExecRestart, Home, Startup, TreeBuilder};
use ignite_core::time::SystemClock;

/// What one daemon is started with.
#[derive(Clone, Debug)]
pub struct DaemonOptions {
    /// The repository's main checkout.
    pub repo: PathBuf,
    /// The machine's Ignite folder: its identity, local database, runs and records.
    pub home: Home,
    /// The machine's name, which names its table in `ignite.toml`.
    pub hostname: String,
    /// The `ignite` executable that supervisors and the cargo shim run.
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

impl Daemon {
    /// Builds every part, registers the machine with the hub if it has no token yet and creates
    /// the build trees. The background loops start with [`Daemon::run_loops`], and serving is
    /// left to the caller.
    pub fn start(options: DaemonOptions) -> Result<Daemon> {
        let base = assemble::Base::read(options)?;
        let machinery = assemble::machinery(&base)?;
        let (checks, landing, idle) = assemble::checking(&base, &machinery.trees)?;
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
            checks,
            landing,
            idle,
            claims: desks.claims.clone(),
            dispatcher: desks.dispatcher.clone(),
            scheduler: machinery.scheduler.clone(),
            sender_db: Mutex::new(Some(base.open()?)),
            home: base.options.home.clone(),
            version: base.options.version.clone(),
            feed_wait_s: base.options.feed_wait_s,
            update: Arc::new(self_update(&base)),
            health: health.clone(),
            reporter: Arc::new(crate::justiciar::Reporter::new(
                base.options.home.root.clone(),
                crate::doctor::Host::hostname(&crate::doctor::NativeHost),
                base.options.version.clone(),
                health,
                desks.dispatcher.clone(),
            )),
        };
        let server = assemble::server(&base, &ctx, &parts, &desks)?;
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
/// folder, apart from the build trees, and restarts with [`ExecRestart`].
fn self_update(base: &assemble::Base) -> selfupdate::SelfUpdate {
    let options = &base.options;
    selfupdate::SelfUpdate {
        own: Commit::new(&options.version).ok(),
        git: base.git.clone(),
        builder: Box::new(TreeBuilder {
            repo: options.repo.clone(),
            work: options.home.root.join("update"),
        }),
        store: options.home.store(),
        master: base.config.repository.release_branch().to_owned(),
        restart: Box::new(ExecRestart),
        ready: Mutex::new(None),
        rounds: std::sync::atomic::AtomicU32::new(0),
    }
}

/// The hidden `ignite daemon` subcommand: runs the daemon until the process ends, for the
/// checkout at `cwd` or, outside one (the login service starts it in `/`), for the checkout
/// `ignite install` recorded. Returns the exit code.
pub fn run_daemon(cwd: &Path) -> i32 {
    let home = Home::from_env();
    // First, before anything this build could fail at: records the start, and goes back to the
    // last-good build if this one crash-loops.
    if let Some(code) = crate::update::guard_start(&home) {
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
    let ignite = std::env::current_exe()
        .and_then(|exe| exe.canonicalize())
        .unwrap_or_default();
    let version = crate::update::ExecutableStore::version_of(&ignite)
        .map_or_else(|| "unknown".to_owned(), |commit| commit.to_string());
    // From here on, a panic anywhere in the daemon leaves a record in the home folder.
    crate::crash::install_hook(home.root.clone(), version.clone());
    let store = home.store();
    let own = Commit::new(&version).ok();
    if let Some(own) = &own
        && let Startup::RollBack { to, reason } = store.startup(own, SystemClock.now())
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
    if let Err(error) = loops::serve_hub_if_asked(&options) {
        eprintln!("ignite: the hub did not start: {error}");
        return 1;
    }
    let daemon = match Daemon::start(options) {
        Ok(daemon) => daemon,
        Err(error) => {
            eprintln!("ignite: the daemon did not start: {error}");
            return 1;
        }
    };
    daemon.run_loops();
    match daemon.serve(&crate::server::default_socket()) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("ignite: cannot serve the socket: {error}");
            1
        }
    }
}

/// `cwd` if it holds an `ignite.toml`, else the checkout recorded in the home folder.
fn checkout(cwd: &Path, home: &Home) -> PathBuf {
    if cwd.join("ignite.toml").is_file() {
        return cwd.to_path_buf();
    }
    std::fs::read_to_string(home.checkout_file())
        .map_or_else(|_| cwd.to_path_buf(), |text| PathBuf::from(text.trim()))
}

fn read_config(repo: &Path) -> Result<Config> {
    let path = repo.join("ignite.toml");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| Error::Invalid(format!("{}: {e}", path.display())))?;
    Config::parse(&path.to_string_lossy(), &text).map_err(|e| Error::Invalid(e.to_string()))
}

/// Every `*.toml` in the repository's manifest directory, parsed together.
fn read_manifest(repo: &Path, config: &Config) -> std::result::Result<Manifest, String> {
    let dir = repo.join(&config.repository.checks);
    let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut files = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_some_and(|e| e == "toml") {
            let text =
                std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            files.push((path, text));
        }
    }
    files.sort();
    Manifest::from_files(&files).map_err(|e| e.to_string())
}
