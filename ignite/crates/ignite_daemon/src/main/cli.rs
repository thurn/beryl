//! CLI mode in the assembled daemon (see "CLI mode" in the design document): the CLI pool over
//! the machine's local database, its turns run as supervised commands, the dispatcher's link
//! to it, and the two loops that follow it.
//!
//! - Every app that `[pool.cli.<app>]` gives a program gets an adapter, whether or not it runs
//!   in CLI mode now: `ignite pool mode` can switch an app without a restart, and the
//!   dispatcher reads the mode on every step.
//! - Before the daemon's socket answers, [`crate::clid::CliDaemon::recover`] starts each turn a
//!   stopped daemon recorded and never started. Turns that run are followed by the loop.
//! - The `ignite-cli` loop runs [`crate::clid::CliDaemon::step`] about once a second.
//! - The `ignite-cli-health` loop runs the probe turns of the apps in CLI mode and sends each
//!   CLI hold's report ([`Health::tick`]). A probe runs a whole turn, so it has a loop of its
//!   own and never holds up the turns of other slots.
//! - The `ignite-cli-dispatch` loop runs [`Dispatcher::tick`] every [`DISPATCH_EVERY`] while
//!   any app is in CLI mode, so idle CLI slots are offered to the hub. A machine whose apps all
//!   run in CLI mode has no dispatcher session, and nothing else asks for its steps once every
//!   worker is idle: before this loop, such a machine stopped taking tickets until some `ignite`
//!   verb happened to run a dispatch step.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use ignite_core::cli::CliApp;
use ignite_core::cli::claude::Claude;
use ignite_core::cli::codex::Codex;
use ignite_core::config::{Config, PoolMode};
use ignite_core::traits::Result;

use super::assemble::{Base, Desks, Machinery};
use super::loops::Parts;
use crate::clid::health::{Health, HealthEffects, probe_model};
use crate::clid::start::TurnLauncher;
use crate::clid::{self, CliAppSetup, CliDaemon, CliSetup, SupervisedTurns};
use crate::dispatchd::Dispatcher;
use crate::pool::Pool;
use crate::server::Context;

/// How long the `ignite-cli` loop may go without finishing a round before `ignite doctor` calls
/// it stuck. A round reads each running turn's log and may release a ticket through the hub.
const CLI_STALLS_AFTER: Duration = Duration::from_mins(10);
/// How often the health loop looks for a probe that is due.
const HEALTH_EVERY: Duration = Duration::from_secs(30);
/// A round of the health loop runs one probe turn per app, each of which its supervisor ends
/// after `clid::health::PROBE_HANG_S`.
const HEALTH_STALLS_AFTER: Duration = Duration::from_mins(30);
/// The name the health loop registers under.
const HEALTH_LOOP: &str = "ignite-cli-health";
/// How often the dispatch loop runs the dispatcher's step. A step asks the hub for tickets only
/// while a worker is idle and a ticket tree is free.
const DISPATCH_EVERY: Duration = Duration::from_secs(10);
/// A dispatch round asks the hub once per group of idle workers and reads each reserved ticket.
const DISPATCH_STALLS_AFTER: Duration = Duration::from_mins(10);
/// The name the dispatch loop registers under.
const DISPATCH_LOOP: &str = "ignite-cli-dispatch";

/// The adapter of each app that `config` gives a CLI program, by name.
fn adapters(config: &Config) -> Vec<Arc<dyn CliApp>> {
    let all: [Arc<dyn CliApp>; 2] = [Arc::new(Claude), Arc::new(Codex)];
    all.into_iter()
        .filter(|app| config.cli_program(app.name()).is_some())
        .collect()
}

/// The CLI pool and what its loops need.
pub struct CliMode {
    daemon: Arc<CliDaemon>,
    health: Arc<Health>,
    apps: Vec<Arc<dyn CliApp>>,
    dispatcher: Arc<Dispatcher>,
    ctx: Context,
}

/// Builds the CLI pool when `ignite.toml` gives any app a CLI program, starts the turns a
/// stopped daemon recorded and never started, and gives the pool to the dispatcher. `None`
/// when no app has a program, so every app runs with its desktop dispatcher.
pub fn cli_mode(
    base: &Base,
    ctx: &Context,
    machinery: &Machinery,
    desks: &Desks,
) -> Result<Option<Arc<CliMode>>> {
    let apps = adapters(&base.config);
    if apps.is_empty() {
        return Ok(None);
    }
    let home = base.options.home.root.clone();
    let checkout = base.options.repo.clone();
    let pool = Arc::new(Pool::new(base.open()?));
    // Turns start without the scheduler: the CLI program itself uses little CPU, and the
    // builds and checks it asks for take their cores through Ignite's verbs.
    let turns = Arc::new(SupervisedTurns::new(
        TurnLauncher::new(
            machinery.supervised.clone(),
            &base.options.ignite,
            home.clone(),
            checkout.clone(),
        ),
        home.clone(),
    ));
    let health = Arc::new(Health::new(
        pool.clone(),
        home,
        base.options.hostname.clone(),
    ));
    let setup = CliSetup {
        checkout,
        apps: apps
            .iter()
            .filter_map(|app| {
                let program = base.config.cli_program(app.name())?.clone();
                let setup = CliAppSetup {
                    app: app.clone(),
                    program,
                    default_model: base
                        .config
                        .pool
                        .model_id(app.name(), "sonnet")
                        .or_else(|| probe_model(&base.config, app.name())),
                };
                Some((app.name().to_owned(), setup))
            })
            .collect::<BTreeMap<_, _>>(),
    };
    let daemon = Arc::new(
        CliDaemon::new(setup, pool, desks.host.clone(), base.clock.clone(), turns)
            .with_effects(Arc::new(HealthEffects {
                health: health.clone(),
                ctx: ctx.clone(),
            }))
            .with_machine(base.machine.clone()),
    );
    match daemon.recover() {
        Ok(0) => {}
        Ok(started) => eprintln!("ignite: started {started} CLI turns a stopped daemon recorded"),
        Err(error) => eprintln!("ignite: CLI turns not recovered at start: {error}"),
    }
    desks
        .dispatcher
        .set_cli(daemon.clone(), &base.options.hostname, &base.config);
    Ok(Some(Arc::new(CliMode {
        daemon,
        health,
        apps,
        dispatcher: desks.dispatcher.clone(),
        ctx: ctx.clone(),
    })))
}

impl CliMode {
    /// Starts the `ignite-cli`, `ignite-cli-health` and `ignite-cli-dispatch` loops.
    pub fn spawn(&self, parts: &Parts) {
        let daemon = self.daemon.clone();
        parts.start_loop(clid::LOOP, CLI_STALLS_AFTER, move |ticker| {
            loop {
                ticker.round(|| {
                    daemon.step().map_err(|error| {
                        eprintln!("ignite: CLI turns: {error}");
                        format!("CLI turns: {error}")
                    })
                });
                thread::sleep(Duration::from_secs_f64(clid::STEP_S));
            }
        });
        let (health, apps, dispatcher, ctx) = (
            self.health.clone(),
            self.apps.clone(),
            self.dispatcher.clone(),
            self.ctx.clone(),
        );
        parts.start_loop(HEALTH_LOOP, HEALTH_STALLS_AFTER, move |ticker| {
            loop {
                ticker.round(|| {
                    // Only the apps in CLI mode now are probed: a probe costs a turn.
                    let running: Vec<&dyn CliApp> = apps
                        .iter()
                        .filter(|app| dispatcher.mode(app.name()).0 == PoolMode::Cli)
                        .map(AsRef::as_ref)
                        .collect();
                    health.tick(&ctx, &running);
                    Ok(())
                });
                thread::sleep(HEALTH_EVERY);
            }
        });
        let (apps, dispatcher, ctx) =
            (self.apps.clone(), self.dispatcher.clone(), self.ctx.clone());
        parts.start_loop(DISPATCH_LOOP, DISPATCH_STALLS_AFTER, move |ticker| {
            loop {
                ticker.round(|| {
                    if apps
                        .iter()
                        .any(|app| dispatcher.mode(app.name()).0 == PoolMode::Cli)
                    {
                        dispatcher.tick(&ctx);
                    }
                    Ok(())
                });
                thread::sleep(DISPATCH_EVERY);
            }
        });
    }
}
