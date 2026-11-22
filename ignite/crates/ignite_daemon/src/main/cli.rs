//! Connects the CLI worker supervisors to the daemon’s launch, health and ticket loops.

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use ignite_core::cli::CliApp;
use ignite_core::traits::Result;

use super::assemble::{Base, Desks, Machinery};
use super::loops::Parts;
use crate::clid::health::{Health, HealthEffects};
use crate::clid::start::TurnLauncher;
use crate::clid::{self, CliDaemon, CliSetup, SupervisedTurns};
use crate::dispatchd::HeadlessDispatch;
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
/// How often the daemon asks for tickets. A step asks the hub for tickets only
/// while a ticket tree is free.
const DISPATCH_EVERY: Duration = Duration::from_secs(10);
/// A dispatch round asks for a batch of workers and reads each reserved ticket.
const DISPATCH_STALLS_AFTER: Duration = Duration::from_mins(10);
/// The name the dispatch loop registers under.
const DISPATCH_LOOP: &str = "ignite-cli-dispatch";

/// The CLI pool and what its loops need.
pub struct CliMode {
    daemon: Arc<CliDaemon>,
    health: Arc<Health>,
    providers: Vec<(String, Arc<dyn CliApp>)>,
    headless_dispatch: Arc<HeadlessDispatch>,
    ctx: Context,
}

/// Builds the CLI pool when `ignite.toml` gives any app a CLI program, starts the turns a
/// stopped daemon recorded and never started, and connects the pool to headless dispatch.
/// Returns `None` when no app has a program.
pub fn cli_mode(
    base: &Base,
    ctx: &Context,
    machinery: &Machinery,
    desks: &Desks,
) -> Result<Option<Arc<CliMode>>> {
    let checkout = base.options.repo.clone();
    let setup = CliSetup::from_config(checkout.clone(), &base.config);
    if setup.apps.is_empty() {
        return Ok(None);
    }
    let providers = setup
        .apps
        .iter()
        .map(|(name, setup)| (name.clone(), setup.app.clone()))
        .collect();
    let home = base.options.home.root.clone();
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
        .headless_dispatch
        .set_cli(daemon.clone(), &base.config);
    Ok(Some(Arc::new(CliMode {
        daemon,
        health,
        providers,
        headless_dispatch: desks.headless_dispatch.clone(),
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
        let (health, apps, ctx) = (
            self.health.clone(),
            self.providers.clone(),
            self.ctx.clone(),
        );
        parts.start_loop(HEALTH_LOOP, HEALTH_STALLS_AFTER, move |ticker| {
            loop {
                ticker.round(|| {
                    let running: Vec<_> = apps
                        .iter()
                        .map(|(name, app)| (name.as_str(), app.as_ref()))
                        .collect();
                    health.tick(&ctx, &running);
                    Ok(())
                });
                thread::sleep(HEALTH_EVERY);
            }
        });
        let (headless_dispatch, ctx) = (self.headless_dispatch.clone(), self.ctx.clone());
        parts.start_loop(DISPATCH_LOOP, DISPATCH_STALLS_AFTER, move |ticker| {
            loop {
                ticker.round(|| {
                    if headless_dispatch.cli_apps().next().is_some() {
                        headless_dispatch.tick(&ctx);
                    }
                    Ok(())
                });
                thread::sleep(DISPATCH_EVERY);
            }
        });
    }
}
