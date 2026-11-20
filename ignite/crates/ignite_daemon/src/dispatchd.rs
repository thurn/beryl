//! The dispatcher endpoint. The dispatcher is one desktop-app session per machine and app whose
//! only job is to make the tool calls the daemon asks for. Its loop is `ignite dispatch --wait`,
//! which blocks until there is an instruction, and `ignite dispatch ack <id> <result>` once the
//! instruction is carried out.
//!
//! While a dispatcher waits, the daemon:
//!
//! - asks the hub's `/next` for a ticket whenever a worker is idle and a ticket tree is free, and
//!   turns the reservation into a `handoff` (clear the worker, set its model and effort, send the
//!   prompt);
//! - gives an instruction again when it is not acknowledged within 2 minutes, and after three
//!   such failures marks the dispatcher down and queues no new hand-offs;
//! - moves a hand-off whose worker refused it to another idle worker, and leaves the refusing
//!   worker alone for a minute;
//! - leaves a worker whose ticket just ended alone for 3 minutes while it finishes its turn;
//! - reads the hub's feed: a new `tell` on a ticket held here becomes a `message` to its session
//!   and ends that session's `ignite wait`;
//! - gives a regression the hub reserved for this machine to the idle worker whose ticket caused
//!   it, if there is one, so that the worker that still holds the change's context fixes it;
//! - asks the dispatcher to `check` a worker that has run no command for 30 minutes, or whose
//!   command has run far past its time (see `stall`), messages it once if it is idle, and
//!   releases its ticket when it is gone or stays idle 15 minutes more;
//! - answers a worker's `Stop` hook through `ignite turn-ended` (see `turn`), which keeps a
//!   worker from ending its turn holding its ticket.
//!
//! What other parts of the daemon own (trees, admission, handles, releasing) reaches this module
//! through [`Host`]. The instruction queue is saved in the local database whenever it changes
//! (see `store`), so after a restart an instruction that was given and not acknowledged is given
//! again at once instead of lapsing at the hub 10 minutes later.

use std::collections::{BTreeMap, VecDeque};
use std::fmt::Write as _;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::{Deserialize, Serialize};

use ignite_core::config::Os;
use ignite_core::ids::{Seq, SessionId, TicketId};
use ignite_core::protocol::client::{DispatchAction, DispatchInstruction, Exit, Reply};
use ignite_core::protocol::hub::{Reservation, WorkerRole};
use ignite_core::protocol::tickets::TicketDetail;
use ignite_core::time::Timestamp;
use ignite_core::traits::{Error, Result};
use ignite_store::local::rows::DispatchEntry;

use crate::pool::{Member, Pool};
use crate::server::Context;

#[path = "dispatchd_allowed.rs"]
mod allowed;
#[path = "dispatchd_feed.rs"]
mod feed;
#[path = "dispatchd_justiciar.rs"]
mod justiciar;
#[path = "dispatchd_raw.rs"]
mod raw;
#[path = "dispatchd_slots.rs"]
mod slots;
#[path = "dispatchd_stall.rs"]
mod stall;
#[path = "dispatchd_store.rs"]
mod store;
#[path = "dispatchd_tell.rs"]
mod tell;
#[path = "dispatchd_turn.rs"]
mod turn;
#[path = "dispatchd_verb.rs"]
mod verb;
#[path = "dispatchd_work.rs"]
mod work;

pub use stall::RunningVerb;
pub use tell::TellVerb;
pub use turn::TurnEndedVerb;
pub use verb::DispatchVerb;

/// Seconds an instruction may go unacknowledged before it is given again.
pub const ACK_WAIT_S: f64 = 120.0;
/// Unacknowledged instructions in a row that mark the dispatcher down.
pub const MAX_FAILURES: u32 = 3;
/// Seconds a worker that refused a hand-off is left alone.
pub const REFUSED_S: f64 = 60.0;
/// Seconds a worker whose ticket just ended is left alone before it is handed another. The
/// session is usually still finishing its turn after `ignite land`, and the app refuses to clear
/// a session that is working: about one hand-off in four failed that way.
pub const SETTLE_S: f64 = 180.0;
/// Seconds without a command after which a working session is checked.
pub const SILENT_S: f64 = 1800.0;
/// Seconds an idle session has, after being asked to continue, before its ticket is released.
pub const IDLE_AFTER_MESSAGE_S: f64 = 900.0;
/// Instructions a Codex dispatcher carries out before it opens a new chat, about the 120,000
/// tokens at which a Claude dispatcher's context is compacted.
pub const ROTATE_AFTER: u32 = 200;
/// The longest single wait on the hub's feed, so a dispatcher's wait notices new work promptly.
pub const FEED_WAIT_S: u32 = 30;

/// The line every instruction ends with, so the dispatcher keeps its loop after compaction.
pub const LOOP: &str = "Loop: run the recipe in AGENTS.md for this `do`, then `ignite dispatch ack \
                        <id> ok|failed <why>|raw '<result>'`, then start `ignite dispatch --wait \
                        3600` again in the background and end your turn.";

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

/// What the dispatcher needs from the rest of the daemon.
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
    /// Whether a command that ends the session's ticket is still running: its current command,
    /// or an operation it started that runs under a handle after its caller stopped waiting.
    /// Other commands (`test`, `check`, `wait`, ...) do not count.
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
    /// The `tier:` label's value, which is what pool members' `models` lists are matched against.
    /// `None` for a ticket that names no tier on an app with no default, and for a reservation
    /// saved before this field existed.
    #[serde(default)]
    tier: Option<String>,
    /// The model ID to ask the app for, from `[pool.models]` in `ignite.toml`.
    model: Option<String>,
    effort: String,
    app: String,
}

/// The reasoning effort of a hand-off whose ticket has no `effort:` label.
const DEFAULT_EFFORT: &str = "medium";

/// Why an instruction was queued, which decides what its acknowledgement does.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
enum Purpose {
    Handoff(Reserved),
    Message,
    Check,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Queued {
    body: DispatchInstruction,
    purpose: Purpose,
    /// Times it has been given.
    gives: u32,
}

#[derive(Clone, Debug)]
struct Given {
    queued: Queued,
    at: Timestamp,
    /// A hand-off's model and effort read back as asked (see `raw`).
    verified: bool,
    /// The dispatcher's hook reported that this hand-off's worker was cleared (see `allowed`).
    cleared: bool,
}

/// A working session that went silent, or whose command is stuck.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Stall {
    checked_at: Timestamp,
    messaged_at: Option<Timestamp>,
    /// Set when the stall is a command stuck past its time rather than silence.
    #[serde(default)]
    stuck: Option<stall::Stuck>,
}

#[derive(Debug, Default)]
struct State {
    next_id: u64,
    pending: VecDeque<Queued>,
    given: BTreeMap<u64, Given>,
    failures: u32,
    down: bool,
    /// The daemon is waiting to update to another version and asks for no new work.
    draining: bool,
    reserved: BTreeMap<SessionId, Reserved>,
    /// Hand-offs whose worker refused them, waiting for another idle worker.
    orphans: Vec<Reserved>,
    /// Reservations whose ticket could not be read yet, to try again on the next tick.
    unread: Vec<(Reservation, String)>,
    refused: BTreeMap<SessionId, Timestamp>,
    /// The worker given a justiciar prompt without a ticket, and when (see `justiciar`).
    justiciar: Option<(SessionId, Timestamp)>,
    stalls: BTreeMap<SessionId, Stall>,
    /// Who held a tree at the last look, to notice who finished.
    held: BTreeMap<SessionId, TicketId>,
    last_ticket: BTreeMap<SessionId, TicketId>,
    /// When each session's ticket last ended, to let it settle (see [`SETTLE_S`]).
    finished_at: BTreeMap<SessionId, Timestamp>,
    feed: Option<Seq>,
    tells: BTreeMap<TicketId, u64>,
    given_to: BTreeMap<SessionId, u32>,
    /// A step is asking the hub for tickets, with the state unlocked.
    asking_hub: bool,
    /// When `ignite pool add --dispatcher` last ran, until its workers are registered (see
    /// `allowed`).
    setup_since: Option<Timestamp>,
    /// When a dispatcher was last told to `rotate` (see `allowed`).
    rotating_since: Option<Timestamp>,
    /// When the Claude dispatcher's hook first reported a clear. From then on hand-offs wait
    /// for one (see `allowed`).
    clears_reported: Option<Timestamp>,
    /// How many workers were left idle after the last request for tickets, and why.
    idle_reason: Option<(usize, String)>,
}

/// The dispatcher's state and the pool, shared by the `dispatch`, `pool` and `tell` verbs.
pub struct Dispatcher {
    pool: Pool,
    host: Arc<dyn Host>,
    setup: Setup,
    state: Mutex<State>,
    /// The rows last written to the database (see `store`).
    saved: Mutex<Vec<DispatchEntry>>,
}

/// The exit code for an error from the hub or the store.
pub fn hub_exit(error: &Error) -> Exit {
    match error {
        Error::Unreachable(_) => Exit::Unreachable,
        Error::Conflict(_) => Exit::Conflict,
        _ => Exit::Usage,
    }
}

/// The tier a ticket runs on: its `tier:` label, or `sonnet` for a Claude worker and nothing for
/// a Codex one, which uses its app's own default model.
fn tier_for(labels: &[String], app: &str) -> Option<String> {
    match label(labels, "tier:") {
        Some(tier) => Some(tier.to_owned()),
        None if app == "claude" => Some("sonnet".to_owned()),
        None => None,
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

fn suits(member: &Member, tier: Option<&String>, app: &str) -> bool {
    member.app == app && tier.is_none_or(|t| member.models.contains(t))
}

impl Dispatcher {
    pub fn new(pool: Pool, host: Arc<dyn Host>, setup: Setup) -> Dispatcher {
        let state = pool
            .load_queue()
            .map(|entries| store::restore(&entries))
            .unwrap_or_default();
        Dispatcher {
            pool,
            host,
            setup,
            state: Mutex::new(state),
            saved: Mutex::default(),
        }
    }

    pub fn pool(&self) -> &Pool {
        &self.pool
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn is_down(&self) -> bool {
        self.state().down
    }

    /// Drops what the dispatcher remembers of a session removed from the pool.
    pub fn forget(&self, session: &SessionId) {
        let mut state = self.state();
        state.reserved.remove(session);
        state.stalls.remove(session);
        state.refused.remove(session);
        state.finished_at.remove(session);
        state
            .pending
            .retain(|q| q.body.session.as_ref() != Some(session));
        drop(state);
        self.save();
    }

    /// Queues a `check` of a session removed from the pool that still holds a ticket, so that
    /// its ticket is released if the session is gone. Without it, a worker of a replaced
    /// dispatcher that died holding a ticket kept it until someone released it by hand.
    pub fn check_holder(&self, session: &SessionId) {
        if !self.host.holders().contains_key(session) {
            return;
        }
        let mut state = self.state();
        if Self::check_outstanding(&state, session) {
            return;
        }
        Self::queue_check(&mut state, session);
        drop(state);
        self.save();
    }

    /// Stops or resumes asking the hub for tickets while the daemon waits to update. Work
    /// already handed to workers carries on.
    pub fn set_draining(&self, draining: bool) {
        self.state().draining = draining;
    }

    /// A landing's next chain ticket with `--fresh`: hand it to the same session, cleared first.
    pub fn chain(&self, ctx: &Context, reservation: &Reservation) {
        let app = self
            .pool
            .member(&reservation.session)
            .ok()
            .flatten()
            .map_or_else(|| "claude".to_owned(), |m| m.app);
        let detail = ctx.hub.ticket(&reservation.ticket).ok();
        let members = self.pool.members().unwrap_or_default();
        let mut state = self.state();
        let now = ctx.clock.now();
        Self::queue_handoff(&ctx.config, &mut state, reservation, &app, detail, &members, now);
        drop(state);
        self.save();
    }

    /// Brings the state up to date and gives the next instruction for the dispatcher `asking`,
    /// if there is one.
    pub fn step(&self, ctx: &Context, asking: Option<&SessionId>) -> Option<DispatchInstruction> {
        let instruction = self.next_instruction(ctx, asking);
        self.save();
        instruction
    }

    fn next_instruction(
        &self,
        ctx: &Context,
        asking: Option<&SessionId>,
    ) -> Option<DispatchInstruction> {
        let members = self.pool.members().unwrap_or_default();
        self.refresh(ctx, &members);
        let mut state = self.state();
        let now = ctx.clock.now();
        Self::drop_expired_handoffs(&mut state, now);
        let asking_app = asking.and_then(|session| {
            members
                .iter()
                .find(|m| &m.session == session && m.role == WorkerRole::Dispatcher)
                .map(|m| m.app.clone())
        });
        if let (Some(session), Some("codex")) = (asking, asking_app.as_deref())
            && state.given_to.get(session).copied().unwrap_or(0) >= ROTATE_AFTER
        {
            state.given_to.remove(session);
            state.rotating_since = Some(now);
            return Some(DispatchInstruction {
                id: 0,
                action: DispatchAction::Rotate,
                session: Some(session.clone()),
                clear: false,
                model: None,
                effort: None,
                prompt: Some(
                    "Your context has grown. Open a new chat in your folder whose prompt is \
                     `/ignite pool 0`, then stop."
                        .to_owned(),
                ),
            });
        }
        let app_of = |session: &Option<SessionId>| {
            session.as_ref().and_then(|s| {
                members
                    .iter()
                    .find(|m| &m.session == s)
                    .map(|m| m.app.clone())
            })
        };
        // A hand-off to a Codex slot names no session, so its app comes from its reservation.
        let index = state.pending.iter().position(|queued| {
            let app = match &queued.purpose {
                Purpose::Handoff(reserved) if !reserved.app.is_empty() => {
                    Some(reserved.app.clone())
                }
                _ => app_of(&queued.body.session),
            };
            asking_app.is_none() || app.is_none_or(|a| Some(a) == asking_app)
        })?;
        let mut queued = state.pending.remove(index)?;
        queued.gives += 1;
        let body = queued.body.clone();
        state.given.insert(
            body.id,
            Given {
                queued,
                at: now,
                verified: false,
                cleared: false,
            },
        );
        if let Some(session) = asking {
            *state.given_to.entry(session.clone()).or_default() += 1;
        }
        Some(body)
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
            self.ticket_changed(&mut state, &ticket, detail.as_ref());
        }
        drop(state);
        self.save();
        Ok(())
    }

    /// Records the acknowledgement of instruction `id`.
    pub fn ack(&self, ctx: &Context, id: u64, result: &str, reason: &str) -> Reply {
        if result == "raw" {
            return self.ack_raw(ctx, id, reason);
        }
        if result == "ok"
            && let Some(refusal) = self.unread_handoff(id)
        {
            return refusal;
        }
        let now = ctx.clock.now();
        let members = self.pool.members().unwrap_or_default();
        let mut state = self.state();
        let queued = match state.given.remove(&id) {
            Some(given) => given.queued,
            None => match state.pending.iter().position(|q| q.body.id == id) {
                Some(index) => state
                    .pending
                    .remove(index)
                    .unwrap_or_else(|| unreachable!()),
                None => {
                    return Reply::text(
                        Exit::Usage,
                        format!("no instruction {id} is waiting for an acknowledgement"),
                    );
                }
            },
        };
        let session = slots::handoff_session(&state, &queued);
        // Any acknowledgement shows the dispatcher is alive; a failed one is the worker's fault.
        state.failures = 0;
        state.down = false;
        let mut text = format!("acked    {id} {result}");
        if result == "failed" {
            justiciar::handoff_failed(&mut state, &queued, now);
        }
        let mut release = None;
        match (&queued.purpose, result, session) {
            (Purpose::Handoff(reserved), "failed", Some(session)) => {
                state.refused.insert(session.clone(), now);
                state.reserved.remove(&session);
                let _ = write!(text, "; {session} is left alone for a minute");
                if !reason.is_empty() {
                    let _ = write!(text, " ({reason})");
                }
                let reserved = reserved.clone();
                let workers = slots::with_slots(&state, &members);
                match Self::idle_workers(&state, &workers, now)
                    .into_iter()
                    .find(|m| suits(m, reserved.tier.as_ref(), &reserved.app))
                {
                    Some(other) => {
                        let _ = write!(text, "; {} goes to {}", reserved.ticket, other.session);
                        let other = other.session.clone();
                        Self::queue_handoff_reserved(&mut state, &other, reserved);
                    }
                    None => state.orphans.push(reserved),
                }
            }
            (Purpose::Handoff(_), "ok", Some(slot)) if slots::is_slot(&slot) => {
                text.push_str(&self.slot_opened(&mut state, &members, &slot, reason, now));
            }
            (Purpose::Check, answer, Some(session)) => {
                let (note, due) = self.checked(&mut state, ctx, &session, answer);
                text.push_str(&note);
                release = due;
            }
            _ => {}
        }
        drop(state);
        self.save();
        // Giving a ticket back asks the hub, so it happens with the state unlocked.
        if let Some(due) = release {
            text.push_str(&self.release(&due));
        }
        Reply::text(Exit::Success, text)
    }

    /// Workers that could take a ticket now.
    fn idle_workers<'a>(state: &State, members: &'a [Member], now: Timestamp) -> Vec<&'a Member> {
        let holders = &state.held;
        members
            .iter()
            .filter(|m| m.role == WorkerRole::Worker && !m.down && !m.single_use)
            .filter(|m| {
                !holders.contains_key(&m.session) && !state.reserved.contains_key(&m.session)
            })
            .filter(|m| !justiciar::holds(state, &m.session, now))
            .filter(|m| {
                state
                    .refused
                    .get(&m.session)
                    .is_none_or(|at| now.seconds_since(*at) >= REFUSED_S)
            })
            .filter(|m| {
                state
                    .finished_at
                    .get(&m.session)
                    .is_none_or(|at| now.seconds_since(*at) >= SETTLE_S)
            })
            .filter(|m| {
                !state
                    .pending
                    .iter()
                    .chain(state.given.values().map(|g| &g.queued))
                    .any(|q| {
                        matches!(q.purpose, Purpose::Handoff(_))
                            && q.body.session.as_ref() == Some(&m.session)
                    })
            })
            .collect()
    }

    /// Gives unacknowledged instructions again, and marks the dispatcher down after three.
    fn expire_given(state: &mut State, now: Timestamp) {
        let overdue: Vec<u64> = state
            .given
            .iter()
            .filter(|(_, g)| now.seconds_since(g.at) >= ACK_WAIT_S)
            .map(|(id, _)| *id)
            .collect();
        for id in overdue {
            let Some(given) = state.given.remove(&id) else {
                continue;
            };
            state.failures += 1;
            if state.failures >= MAX_FAILURES {
                state.down = true;
            }
            if given.queued.gives >= MAX_FAILURES {
                justiciar::handoff_failed(state, &given.queued, now);
                if let (Purpose::Handoff(_), Some(session)) = (
                    &given.queued.purpose,
                    slots::handoff_session(state, &given.queued),
                ) {
                    state.reserved.remove(&session);
                }
            } else {
                state.pending.push_front(given.queued);
            }
        }
    }

    fn retry_orphans(state: &mut State, members: &[Member], now: Timestamp) {
        let orphans = std::mem::take(&mut state.orphans);
        for reserved in orphans {
            let workers = slots::with_slots(state, members);
            let worker = Self::idle_workers(state, &workers, now)
                .into_iter()
                .find(|m| suits(m, reserved.tier.as_ref(), &reserved.app))
                .map(|m| m.session.clone());
            match worker {
                Some(session) => Self::queue_handoff_reserved(state, &session, reserved),
                None => state.orphans.push(reserved),
            }
        }
    }

    /// Queues the hand-off of `reserved` to `session`. A Codex slot's hand-off names no session
    /// and clears nothing: the dispatcher opens a new chat for it (see `slots`).
    fn queue_handoff_reserved(state: &mut State, session: &SessionId, reserved: Reserved) {
        let prompt = format!(
            "Run `ignite start {}` and follow what it prints.",
            start_arguments(&reserved)
        );
        let clear = !slots::is_slot(session);
        Self::push_handoff(state, session, reserved, prompt, clear);
    }

    /// Queues the hand-off of `reserved`, a regression, to `session`, the idle worker whose last
    /// ticket caused it. The worker is not cleared, so it keeps the conversation in which it made
    /// the change that broke the check.
    fn queue_regression_handoff(
        state: &mut State,
        session: &SessionId,
        reserved: Reserved,
        title: &str,
    ) {
        let prompt = format!(
            "{} is a regression caused by your last ticket: {title}. Run `ignite start {}` and \
             fix it.",
            reserved.ticket,
            start_arguments(&reserved)
        );
        Self::push_handoff(state, session, reserved, prompt, false);
    }

    fn push_handoff(
        state: &mut State,
        session: &SessionId,
        reserved: Reserved,
        prompt: String,
        clear: bool,
    ) {
        state.reserved.insert(session.clone(), reserved.clone());
        let id = Self::next_id(state);
        let slot = slots::is_slot(session);
        state.pending.push_back(Queued {
            body: DispatchInstruction {
                id,
                action: DispatchAction::Handoff,
                session: (!slot).then(|| session.clone()),
                clear,
                model: reserved.model.clone(),
                effort: Some(reserved.effort.clone()),
                prompt: Some(prompt),
            },
            purpose: Purpose::Handoff(reserved),
            gives: 0,
        });
    }

    fn queue_check(state: &mut State, session: &SessionId) {
        let prompt = format!(
            "Call get_session on {session} and report what it returned, JSON or error, as it \
             came. In Codex, answer running, idle or gone: an archived session is `gone`, and a \
             lookup that failed is `unknown`, never `gone`."
        );
        Self::queue(
            state,
            Some(session),
            DispatchAction::Check,
            &prompt,
            Purpose::Check,
        );
    }

    fn queue(
        state: &mut State,
        session: Option<&SessionId>,
        action: DispatchAction,
        text: &str,
        purpose: Purpose,
    ) {
        let id = Self::next_id(state);
        state.pending.push_back(Queued {
            body: DispatchInstruction {
                id,
                action,
                session: session.cloned(),
                clear: false,
                model: None,
                effort: None,
                prompt: Some(text.to_owned()),
            },
            purpose,
            gives: 0,
        });
    }

    fn next_id(state: &mut State) -> u64 {
        state.next_id += 1;
        state.next_id
    }

    /// Whether a change to `ticket` needs its details from the hub: it is held by a session
    /// here. A regression is not read here: the hub alone decides which machine works it, and
    /// its reservation reaches this machine through `/next` (see `work`).
    fn cares_about(state: &State, ticket: &TicketId) -> bool {
        state.held.values().any(|held| held == ticket)
    }

    /// Acts on a change to `ticket`, whose `detail` the caller read from the hub, if it could.
    fn ticket_changed(&self, state: &mut State, ticket: &TicketId, detail: Option<&TicketDetail>) {
        if let Some(holder) = state
            .held
            .iter()
            .find(|(_, t)| *t == ticket)
            .map(|(s, _)| s.clone())
            && let Some(detail) = detail
        {
            self.deliver_tell(state, &holder, detail);
        }
    }

    fn deliver_tell(&self, state: &mut State, holder: &SessionId, detail: &TicketDetail) {
        feed::deliver_tell(self, state, holder, detail);
    }
}

#[cfg(test)]
#[path = "dispatchd_allowed_tests.rs"]
mod allowed_tests;
#[cfg(test)]
#[path = "dispatchd_hub_tests.rs"]
mod hub_tests;
#[cfg(test)]
#[path = "dispatchd_justiciar_tests.rs"]
mod justiciar_tests;
#[cfg(test)]
#[path = "dispatchd_models_tests.rs"]
mod models_tests;
#[cfg(test)]
#[path = "dispatchd_raw_tests.rs"]
mod raw_tests;
#[cfg(test)]
#[path = "dispatchd_regression_tests.rs"]
mod regression_tests;
#[cfg(test)]
#[path = "dispatchd_restart_tests.rs"]
mod restart_tests;
#[cfg(test)]
#[path = "dispatchd_slots_tests.rs"]
mod slots_tests;
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
