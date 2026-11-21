//! Reserves tickets with the hub and starts CLI workers on this machine.
//! The hub feed delivers messages to the worker holding the affected ticket.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use serde::{Deserialize, Serialize};

use ignite_core::config::Os;
use ignite_core::ids::{Seq, SessionId, TicketId};
use ignite_core::protocol::client::{Exit, Reply};
use ignite_core::protocol::hub::Reservation;
use ignite_core::protocol::tickets::TicketDetail;
use ignite_core::time::Timestamp;
use ignite_core::traits::{Error, Result};
use ignite_store::local::rows::CheckSighting;

use crate::pool::Pool;
use crate::server::Context;

#[path = "dispatchd_cli.rs"]
mod cli;
#[path = "dispatchd_cli_view.rs"]
mod cli_view;
#[path = "dispatchd_feed.rs"]
mod feed;
#[path = "dispatchd_recovery.rs"]
mod recovery;
#[path = "dispatchd_shared.rs"]
mod shared;
#[path = "dispatchd_stall.rs"]
mod stall;
#[path = "dispatchd_tell.rs"]
mod tell;
#[path = "dispatchd_turn.rs"]
mod turn;
#[path = "dispatchd_usage.rs"]
mod usage;
#[path = "dispatchd_wake.rs"]
mod wake;
#[path = "dispatchd_work.rs"]
mod work;
#[path = "dispatchd_workers.rs"]
mod workers;

pub use cli_view::digest_text;
pub use stall::RunningVerb;
pub use tell::TellVerb;
pub use turn::TurnEndedVerb;
pub use usage::{hold_lines, post_usage, usage_holds, usage_report};

/// Seconds a worker that refused a hand-off is left alone.
pub const REFUSED_S: f64 = 60.0;
/// Seconds without a command after which a working session is checked.
pub const SILENT_S: f64 = 1800.0;
/// Seconds an idle session has, after being asked to continue, before its ticket is released.
pub const IDLE_AFTER_MESSAGE_S: f64 = 900.0;

/// The message an idle worker gets when it has run nothing for 30 minutes.
pub const CONTINUE: &str = "You have run no `ignite` command for 30 minutes. Continue the \
                            ticket, or run `ignite release` with its exact state.";

/// The `ignite` command a session is running or last ran.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionCommand {
    /// The verb and its first argument, such as `wait L-5521`.
    pub command: String,
    /// When it started, or when it ended if it is no longer running.
    pub since: Timestamp,
    pub running: bool,
}

/// The verbs that end a ticket. Finishing one of them reports on the ticket or hands it back,
/// so a worker that stops while one runs in the background is not abandoning the ticket.
pub const TICKET_ENDING_VERBS: &[&str] = &["land", "release", "close"];

/// Whether `command` (a verb and its first argument, as in [`SessionCommand`]) ends a ticket.
pub fn ends_ticket(command: &str) -> bool {
    command
        .split_whitespace()
        .next()
        .is_some_and(|verb| TICKET_ENDING_VERBS.contains(&verb))
}

/// The daemon resources and session state used when assigning tickets.
pub trait Host: Send + Sync {
    /// Ticket trees free to lease.
    fn free_ticket_trees(&self) -> u32;
    /// The cores the scheduler has free at this moment.
    fn free_cores(&self) -> f64;
    /// The cores the machine admits tickets against: all the cores its scheduler hands out. The
    /// hub subtracts the expected cores of the tickets in progress here.
    fn admit_cores(&self) -> f64;
    /// How long the scheduler's oldest request of ticket work (the `land` and `agent` classes)
    /// has waited for cores, in seconds; zero when none waits.
    fn ticket_work_wait(&self) -> f64;
    /// Observe the core queue even when all worker slots are busy.
    fn observe_queue(&self, _now: Timestamp) {}
    /// Ask the disk loop to make room for a queued ticket by clearing unused outputs.
    fn reclaim_for_handoff(&self, _target_gb: u64) {}
    /// The disk and queue conditions local to this machine.
    fn pressure(
        &self,
        _now: Timestamp,
    ) -> Option<ignite_core::dispatch_pressure::DispatchPressure> {
        None
    }
    /// The ticket each session holds a build tree for.
    fn holders(&self) -> BTreeMap<SessionId, TicketId>;
    fn command(&self, session: &SessionId) -> Option<SessionCommand>;
    /// When a session that holds a ticket tree last ran a command, from a clock that is saved
    /// across daemon restarts. [`Host::command`] comes from memory, so it knows nothing of a
    /// session whose last command ran before the daemon started.
    fn quiet_since(&self, session: &SessionId) -> Option<Timestamp>;
    /// The verbs the session started that are still running, whether or not it still waits on
    /// them, with their current steps.
    fn running_verbs(&self, session: &SessionId) -> Vec<RunningVerb>;
    /// Ends the session's `ignite wait` at once with `reply`. Returns whether it was waiting.
    fn interrupt(&self, session: &SessionId, reply: Reply) -> bool;
    /// Gives back the ticket of a session that is gone or idle, keeping its work.
    fn release(&self, session: &SessionId, ticket: &TicketId, reason: &str) -> Result<()>;
    /// The title the desktop app shows for the session, or `None` when it cannot be read.
    fn title(&self, session: &SessionId) -> Option<String>;
    /// The live processes under process `root` (the session's own process, as its `Stop` hook
    /// reports it), leaving out the branch that holds `asker` (the hook's own client), or
    /// `None` when they cannot be read.
    fn running_under(&self, root: u32, asker: Option<u32>) -> Option<Vec<u32>>;
    /// Whether a command that ends the session's ticket is still running: its current command,
    /// or an operation it started that runs under a handle after its caller stopped waiting.
    /// Other commands (`test`, `check`, `wait`, ...) do not count.
    /// Every session's latest outcome of every check on this machine recorded at or after
    /// `since`, oldest first (see `shared`).
    fn check_sightings(&self, since: Timestamp) -> Vec<CheckSighting>;
    /// The commit the `green` branch points at; `None` when there is none (see `shared`).
    fn green(&self) -> Option<String>;
    fn ending_ticket(&self, session: &SessionId) -> bool {
        self.command(session)
            .is_some_and(|command| command.running && ends_ticket(&command.command))
    }
}

/// Facts about the machine that `/next` needs.
#[derive(Clone, Debug)]
pub struct Setup {
    pub os: Os,
    /// The check families the machine runs; empty means every family.
    pub families: Vec<String>,
}

/// A ticket reserved for a worker here and not yet claimed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Reserved {
    ticket: TicketId,
    expires_at: Timestamp,
    /// The hub's one-time code for the reservation, which the hand-off's `ignite start` sends.
    /// `None` for a ticket the daemon points a worker at without a hub reservation.
    code: Option<String>,
    /// The model ID to ask the app for, from `[pool.models]` in `ignite.toml`.
    model: Option<String>,
    effort: String,
    app: String,
}

/// A working session that went silent, or whose command is stuck.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Stall {
    checked_at: Timestamp,
    messaged_at: Option<Timestamp>,
    /// Set when the stall is a command stuck past its time rather than silence.
    #[serde(default)]
    stuck: Option<stall::Stuck>,
    /// Set when the session ended its turn waiting on an `ignite` verb that has since finished
    /// (see `wake`), so an idle answer gets the wake-up message.
    #[serde(default)]
    woken: bool,
}

#[derive(Debug, Default)]
struct State {
    /// The daemon is waiting to update to another version and asks for no new work.
    draining: bool,
    reserved: BTreeMap<SessionId, Reserved>,
    /// Reservations whose ticket could not be read yet, to try again on the next tick.
    unread: Vec<(Reservation, String)>,
    refused: BTreeMap<SessionId, Timestamp>,
    /// The CLI justiciar started without a ticket and its start time.
    justiciar: Option<(SessionId, Timestamp)>,
    stalls: BTreeMap<SessionId, Stall>,
    /// Sessions whose ticket Ignite released (see `wake`), which may not start another ticket
    /// until the daemon has reserved a successor for them.
    released: BTreeMap<SessionId, TicketId>,
    /// Sessions that ended their turn waiting on a running `ignite` verb, and when (see `wake`).
    waiting: BTreeMap<SessionId, Timestamp>,
    /// Who held a tree at the last look, to notice who finished.
    held: BTreeMap<SessionId, TicketId>,
    feed: Option<Seq>,
    tells: BTreeMap<TicketId, u64>,
    /// A step is asking the hub for tickets, with the state unlocked.
    asking_hub: bool,
    /// How many workers were left idle after the last request for tickets, and why.
    idle_reason: Option<(usize, String)>,
    /// Hand-offs, messages and checks for CLI slots and sessions, to carry out once the state
    /// is unlocked (see `cli`).
    cli_calls: Vec<cli::Call>,
    /// The `Begin` calls taken from `cli_calls` that have not finished running (see `cli`).
    begins_running: Vec<cli::Call>,
    /// The apps in CLI mode at the last step, with their slots (see `cli`).
    cli_layout: Vec<cli::Layout>,
    /// The shared failure in force, if any (see `shared`).
    shared: shared::Watch,
}

/// The state for reserving tickets and supervising the local CLI pool.
pub struct Dispatcher {
    pool: Pool,
    host: Arc<dyn Host>,
    setup: Setup,
    state: Mutex<State>,
    /// The last recovery snapshot written to the database.
    saved: Mutex<Option<String>>,
    /// The local CLI supervisor, attached during startup.
    cli: OnceLock<cli::CliMode>,
}

/// The exit code for an error from the hub or the store.
pub fn hub_exit(error: &Error) -> Exit {
    match error {
        Error::Unreachable(_) => Exit::Unreachable,
        Error::Conflict(_) => Exit::Conflict,
        _ => Exit::Usage,
    }
}

fn label<'a>(labels: &'a [String], prefix: &str) -> Option<&'a str> {
    labels.iter().find_map(|l| l.strip_prefix(prefix))
}

/// What follows `ignite start` in a hand-off: the ticket and, when the hub reserved it, the
/// reservation's code.
fn start_arguments(reserved: &Reserved) -> String {
    match &reserved.code {
        Some(code) => format!("{} --reservation {code}", reserved.ticket),
        None => reserved.ticket.to_string(),
    }
}

impl Dispatcher {
    pub fn new(pool: Pool, host: Arc<dyn Host>, setup: Setup) -> Dispatcher {
        Dispatcher {
            state: Mutex::new(recovery::load(&pool)),
            pool,
            host,
            setup,
            saved: Mutex::default(),
            cli: OnceLock::new(),
        }
    }

    pub fn pool(&self) -> &Pool {
        &self.pool
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Stops or resumes asking the hub for tickets while the daemon waits to update. Work
    /// already handed to workers carries on.
    pub fn set_draining(&self, draining: bool) {
        self.state().draining = draining;
    }

    /// Reads the hub's feed for up to `wait_s` seconds and acts on what changed. The hub is
    /// asked about tickets with the state unlocked.
    pub fn poll_feed(&self, ctx: &Context, wait_s: u32) -> Result<()> {
        let since = self.state().feed;
        let Some(since) = since else {
            // The first look only finds where the feed is: what happened before the daemon
            // started is already in each ticket's comments.
            let batch = ctx.hub.feed(Seq(u64::MAX), 0)?;
            self.state().feed = Some(batch.seq);
            return Ok(());
        };
        let batch = ctx.hub.feed(since, wait_s)?;
        let mut tickets: Vec<TicketId> = Vec::new();
        for change in &batch.changes {
            if let Some(ticket) = &change.ticket
                && !tickets.contains(ticket)
            {
                tickets.push(ticket.clone());
            }
        }
        let wanted: Vec<TicketId> = {
            let mut state = self.state();
            if batch.seq < since {
                // The hub's journal restarted at a lower position, as when its database was
                // restored from a backup: a cursor that stayed at the old, higher position
                // would never see another change.
                eprintln!(
                    "ignite: the hub's feed position went back from {} to {}; following it",
                    since.0, batch.seq.0
                );
            }
            state.feed = Some(batch.seq);
            tickets
                .into_iter()
                .filter(|ticket| Self::cares_about(&state, ticket))
                .collect()
        };
        let details: Vec<(TicketId, Option<TicketDetail>)> = wanted
            .into_iter()
            .map(|ticket| {
                let detail = ctx.hub.ticket(&ticket).ok();
                (ticket, detail)
            })
            .collect();
        let mut state = self.state();
        for (ticket, detail) in details {
            Self::ticket_changed(&mut state, &ticket, detail.as_ref());
        }
        drop(state);
        self.save();
        self.run_cli_calls(ctx);
        Ok(())
    }

    /// Whether a change to `ticket` needs its details from the hub: it is held by a session
    /// here. A regression is not read here: the hub alone decides which machine works it, and
    /// its reservation reaches this machine through `/next` (see `work`).
    fn cares_about(state: &State, ticket: &TicketId) -> bool {
        state.held.values().any(|held| held == ticket)
    }

    /// Acts on a change to `ticket`, whose `detail` the caller read from the hub, if it could.
    fn ticket_changed(state: &mut State, ticket: &TicketId, detail: Option<&TicketDetail>) {
        if let Some(holder) = state
            .held
            .iter()
            .find(|(_, t)| *t == ticket)
            .map(|(s, _)| s.clone())
            && let Some(detail) = detail
        {
            feed::deliver_tell(state, &holder, detail);
        }
    }
}

#[cfg(test)]
#[path = "dispatchd_cli_tests.rs"]
mod cli_tests;
#[cfg(test)]
#[path = "dispatchd_cli_view_tests.rs"]
mod cli_view_tests;
#[cfg(test)]
#[path = "dispatchd_shared_tests.rs"]
mod dispatchd_shared_tests;
#[cfg(test)]
#[path = "dispatchd_hold_tests.rs"]
mod hold_tests;
#[cfg(test)]
#[path = "dispatchd_hub_tests.rs"]
mod hub_tests;
#[cfg(test)]
#[path = "dispatchd_regression_tests.rs"]
mod regression_tests;
#[cfg(test)]
#[path = "dispatchd_reporter_tests.rs"]
mod reporter_tests;
#[cfg(test)]
#[path = "dispatchd_restart_tests.rs"]
mod restart_tests;
#[cfg(test)]
#[path = "dispatchd_stall_tests.rs"]
mod stall_tests;
#[cfg(test)]
#[path = "dispatchd_tell_tests.rs"]
mod tell_tests;
#[cfg(test)]
#[path = "dispatchd_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "dispatchd_turn_tests.rs"]
mod turn_tests;
#[cfg(test)]
#[path = "dispatchd_wake_tests.rs"]
mod wake_tests;
