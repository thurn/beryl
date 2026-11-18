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
//! - reads the hub's feed: a new `tell` on a ticket held here becomes a `message` to its session
//!   and ends that session's `ignite wait`; a regression whose culprit's worker is still idle
//!   here becomes a `message` to that worker;
//! - asks the dispatcher to `check` a worker that has run no command for 30 minutes, messages it
//!   once if it is idle, and releases its ticket when it is gone or stays idle 15 minutes more.
//!
//! What other parts of the daemon own (trees, admission, handles, releasing) reaches this module
//! through [`Host`]. The instruction queue lives in memory: after a restart, a hand-off that was
//! lost lapses at the hub after 10 minutes and the ticket is offered again.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write as _;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use ignite_core::config::Os;
use ignite_core::ids::{Seq, SessionId, TicketId};
use ignite_core::protocol::client::{DispatchAction, DispatchInstruction, Exit, Reply};
use ignite_core::protocol::hub::{IdleWorker, MachineFree, Reservation, WorkerRole, WorkerState};
use ignite_core::protocol::tickets::{Status, TicketDetail};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Error, Result};

use crate::pool::{Member, Pool};
use crate::server::Context;

#[path = "dispatchd_feed.rs"]
mod feed;
#[path = "dispatchd_stall.rs"]
mod stall;
#[path = "dispatchd_tell.rs"]
mod tell;
#[path = "dispatchd_verb.rs"]
mod verb;

pub use tell::TellVerb;
pub use verb::DispatchVerb;

/// Seconds an instruction may go unacknowledged before it is given again.
pub const ACK_WAIT_S: f64 = 120.0;
/// Unacknowledged instructions in a row that mark the dispatcher down.
pub const MAX_FAILURES: u32 = 3;
/// Seconds a worker that refused a hand-off is left alone.
pub const REFUSED_S: f64 = 60.0;
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
pub const LOOP: &str = "Carry it out, report `ignite dispatch ack <id> ok` (or `failed <why>`, \
                        or for a check `running|idle|gone|unknown`), then run \
                        `ignite dispatch --wait 540` again. That loop is your whole job.";

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

/// What the dispatcher needs from the rest of the daemon.
pub trait Host: Send + Sync {
    /// Ticket trees free to lease.
    fn free_ticket_trees(&self) -> u32;
    /// Cores the machine can admit beside the tickets in progress (see `admission`).
    fn free_cores(&self) -> f64;
    /// The ticket each session holds a build tree for.
    fn holders(&self) -> BTreeMap<SessionId, TicketId>;
    fn command(&self, session: &SessionId) -> Option<SessionCommand>;
    /// Ends the session's `ignite wait` at once with `reply`. Returns whether it was waiting.
    fn interrupt(&self, session: &SessionId, reply: Reply) -> bool;
    /// Gives back the ticket of a session that is gone or idle, keeping its work.
    fn release(&self, session: &SessionId, ticket: &TicketId, reason: &str) -> Result<()>;
}

/// Facts about the machine that `/next` needs.
#[derive(Clone, Debug)]
pub struct Setup {
    pub os: Os,
    /// The check families the machine runs; empty means every family.
    pub families: Vec<String>,
}

/// A ticket reserved for a worker here and not yet claimed.
#[derive(Clone, Debug, PartialEq)]
struct Reserved {
    ticket: TicketId,
    expires_at: Timestamp,
    /// The hub's one-time code for the reservation, which the hand-off's `ignite start` sends.
    /// `None` for a ticket the daemon points a worker at without a hub reservation.
    code: Option<String>,
    model: Option<String>,
    effort: String,
    app: String,
}

/// Why an instruction was queued, which decides what its acknowledgement does.
#[derive(Clone, Debug, PartialEq)]
enum Purpose {
    Handoff(Reserved),
    Message,
    Check,
}

#[derive(Clone, Debug, PartialEq)]
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
}

/// A working session that went silent.
#[derive(Clone, Copy, Debug)]
struct Stall {
    checked_at: Timestamp,
    messaged_at: Option<Timestamp>,
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
    stalls: BTreeMap<SessionId, Stall>,
    /// Who held a tree at the last look, to notice who finished.
    held: BTreeMap<SessionId, TicketId>,
    last_ticket: BTreeMap<SessionId, TicketId>,
    feed: Option<Seq>,
    tells: BTreeMap<TicketId, u64>,
    seen: BTreeSet<TicketId>,
    given_to: BTreeMap<SessionId, u32>,
}

/// The dispatcher's state and the pool, shared by the `dispatch`, `pool` and `tell` verbs.
pub struct Dispatcher {
    pool: Pool,
    host: Arc<dyn Host>,
    setup: Setup,
    state: Mutex<State>,
}

/// The exit code for an error from the hub or the store.
pub fn hub_exit(error: &Error) -> Exit {
    match error {
        Error::Unreachable(_) => Exit::Unreachable,
        Error::Conflict(_) => Exit::Conflict,
        _ => Exit::Usage,
    }
}

/// The model to set for a ticket: its `tier:` label (`light` runs on `sonnet`), or `sonnet` for
/// a Claude worker and the app's own default for a Codex one.
fn model_for(labels: &[String], app: &str) -> Option<String> {
    match label(labels, "tier:") {
        Some("light") => Some("sonnet".to_owned()),
        Some(tier) => Some(tier.to_owned()),
        None if app == "claude" => Some("sonnet".to_owned()),
        None => None,
    }
}

fn label<'a>(labels: &'a [String], prefix: &str) -> Option<&'a str> {
    labels.iter().find_map(|l| l.strip_prefix(prefix))
}

fn suits(member: &Member, model: Option<&String>, app: &str) -> bool {
    member.app == app && model.is_none_or(|m| member.models.contains(m))
}

impl Dispatcher {
    pub fn new(pool: Pool, host: Arc<dyn Host>, setup: Setup) -> Dispatcher {
        Dispatcher {
            pool,
            host,
            setup,
            state: Mutex::default(),
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
        state
            .pending
            .retain(|q| q.body.session.as_ref() != Some(session));
    }

    /// Every member as the hub's heartbeat and `ignite pool list` show it.
    /// Stops or resumes asking the hub for tickets while the daemon waits to update. Work
    /// already handed to workers carries on.
    pub fn set_draining(&self, draining: bool) {
        self.state().draining = draining;
    }

    pub fn view(&self) -> Result<Vec<WorkerState>> {
        let members = self.pool.members()?;
        let holders = self.host.holders();
        let state = self.state();
        Ok(members
            .iter()
            .map(|member| {
                let held = holders.get(&member.session);
                let reserved = state.reserved.get(&member.session);
                let status = if member.down {
                    "down"
                } else if held.is_some() {
                    match state.stalls.get(&member.session) {
                        Some(stall) if stall.messaged_at.is_some() => "stalled",
                        _ => "working",
                    }
                } else if reserved.is_some() {
                    "reserved"
                } else {
                    "idle"
                };
                let command = self.host.command(&member.session);
                WorkerState {
                    session: member.session.clone(),
                    app: member.app.clone(),
                    role: member.role,
                    state: status.to_owned(),
                    ticket: held.or(reserved.map(|r| &r.ticket)).cloned(),
                    files: member.files.clone(),
                    command: command.as_ref().map(|c| c.command.clone()),
                    command_since: command.as_ref().map(|c| c.since),
                    command_running: command.is_some_and(|c| c.running),
                }
            })
            .collect())
    }

    /// A landing's next chain ticket with `--fresh`: hand it to the same session, cleared first.
    pub fn chain(&self, ctx: &Context, reservation: &Reservation) {
        let app = self
            .pool
            .member(&reservation.session)
            .ok()
            .flatten()
            .map_or_else(|| "claude".to_owned(), |m| m.app);
        let mut state = self.state();
        Self::queue_handoff(ctx, &mut state, reservation, &app);
    }

    /// Brings the state up to date and gives the next instruction for the dispatcher `asking`,
    /// if there is one.
    pub fn step(&self, ctx: &Context, asking: Option<&SessionId>) -> Option<DispatchInstruction> {
        let members = self.pool.members().unwrap_or_default();
        let mut state = self.state();
        self.tick(ctx, &mut state, &members);
        let now = ctx.clock.now();
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
        let index = state.pending.iter().position(|queued| {
            asking_app.is_none()
                || app_of(&queued.body.session).is_none_or(|a| Some(a) == asking_app)
        })?;
        let mut queued = state.pending.remove(index)?;
        queued.gives += 1;
        let body = queued.body.clone();
        state.given.insert(body.id, Given { queued, at: now });
        if let Some(session) = asking {
            *state.given_to.entry(session.clone()).or_default() += 1;
        }
        Some(body)
    }

    /// Reads the hub's feed for up to `wait_s` seconds and acts on what changed.
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
        let members = self.pool.members()?;
        let mut state = self.state();
        state.feed = Some(batch.seq.max(since));
        for ticket in &tickets {
            self.ticket_changed(ctx, &mut state, &members, ticket);
        }
        Ok(())
    }

    /// Records the acknowledgement of instruction `id`.
    pub fn ack(&self, ctx: &Context, id: u64, result: &str, reason: &str) -> Reply {
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
        let session = queued.body.session.clone();
        // Any acknowledgement shows the dispatcher is alive; a failed one is the worker's fault.
        state.failures = 0;
        state.down = false;
        let mut text = format!("acked    {id} {result}");
        match (&queued.purpose, result, session) {
            (Purpose::Handoff(reserved), "failed", Some(session)) => {
                state.refused.insert(session.clone(), now);
                state.reserved.remove(&session);
                let _ = write!(text, "; {session} is left alone for a minute");
                if !reason.is_empty() {
                    let _ = write!(text, " ({reason})");
                }
                let reserved = reserved.clone();
                match Self::idle_workers(&state, &members, now)
                    .into_iter()
                    .find(|m| suits(m, reserved.model.as_ref(), &reserved.app))
                {
                    Some(other) => {
                        let _ = write!(text, "; {} goes to {}", reserved.ticket, other.session);
                        let other = other.session.clone();
                        Self::queue_handoff_reserved(&mut state, &other, reserved);
                    }
                    None => state.orphans.push(reserved),
                }
            }
            (Purpose::Check, answer, Some(session)) => {
                text.push_str(&self.checked(&mut state, ctx, &session, answer));
            }
            _ => {}
        }
        Reply::text(Exit::Success, text)
    }

    /// Workers that could take a ticket now.
    fn idle_workers<'a>(state: &State, members: &'a [Member], now: Timestamp) -> Vec<&'a Member> {
        let holders = &state.held;
        members
            .iter()
            .filter(|m| m.role == WorkerRole::Worker && !m.down)
            .filter(|m| {
                !holders.contains_key(&m.session) && !state.reserved.contains_key(&m.session)
            })
            .filter(|m| {
                state
                    .refused
                    .get(&m.session)
                    .is_none_or(|at| now.seconds_since(*at) >= REFUSED_S)
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

    fn tick(&self, ctx: &Context, state: &mut State, members: &[Member]) {
        let now = ctx.clock.now();
        self.notice_holders(ctx, state, now);
        for (reservation, app) in std::mem::take(&mut state.unread) {
            if reservation.expires_at > now && state.reserved.contains_key(&reservation.session) {
                Self::queue_handoff(ctx, state, &reservation, &app);
            }
        }
        Self::expire_given(state, now);
        self.watch_silence(state, now);
        Self::retry_orphans(state, members, now);
        self.ask_for_work(ctx, state, members, now);
    }

    /// Notices claims, finished tickets and lapsed reservations.
    fn notice_holders(&self, ctx: &Context, state: &mut State, now: Timestamp) {
        let holders = self.host.holders();
        for (session, ticket) in &state.held {
            if !holders.contains_key(session) {
                state.last_ticket.insert(session.clone(), ticket.clone());
                state.stalls.remove(session);
                if let Ok(detail) = ctx.hub.ticket(ticket)
                    && let Some(landed) = detail.ticket.landed
                {
                    let _ = self.pool.update(session, |m| m.files = landed.files);
                }
            }
        }
        let lapsed: Vec<SessionId> = state
            .reserved
            .iter()
            .filter(|(session, r)| !holders.contains_key(*session) && r.expires_at <= now)
            .map(|(session, _)| session.clone())
            .collect();
        state
            .reserved
            .retain(|session, _| !holders.contains_key(session));
        for session in lapsed {
            // A reservation nobody claimed: the worker is treated as stalled.
            state.reserved.remove(&session);
            Self::queue_check(state, &session);
        }
        state.orphans.retain(|r| r.expires_at > now);
        state.held = holders;
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
                if let (Purpose::Handoff(_), Some(session)) =
                    (&given.queued.purpose, &given.queued.body.session)
                {
                    state.reserved.remove(session);
                }
            } else {
                state.pending.push_front(given.queued);
            }
        }
    }

    fn retry_orphans(state: &mut State, members: &[Member], now: Timestamp) {
        let orphans = std::mem::take(&mut state.orphans);
        for reserved in orphans {
            let worker = Self::idle_workers(state, members, now)
                .into_iter()
                .find(|m| suits(m, reserved.model.as_ref(), &reserved.app))
                .map(|m| m.session.clone());
            match worker {
                Some(session) => Self::queue_handoff_reserved(state, &session, reserved),
                None => state.orphans.push(reserved),
            }
        }
    }

    /// Asks the hub for tickets while workers are idle and ticket trees are free.
    fn ask_for_work(&self, ctx: &Context, state: &mut State, members: &[Member], now: Timestamp) {
        if state.down || state.draining || self.pool.paused().unwrap_or(true) {
            return;
        }
        let mut idle: Vec<&Member> = Self::idle_workers(state, members, now);
        let reserved = u32::try_from(state.reserved.len()).unwrap_or(u32::MAX);
        let mut trees = self.host.free_ticket_trees().saturating_sub(reserved);
        while !idle.is_empty() && trees > 0 {
            let free = MachineFree {
                machine: ctx.machine.clone(),
                os: self.setup.os,
                idle_workers: idle
                    .iter()
                    .map(|m| IdleWorker {
                        session: m.session.clone(),
                        app: m.app.clone(),
                        models: m.models.clone(),
                    })
                    .collect(),
                free_ticket_trees: trees,
                free_cores: self.host.free_cores(),
                families: self.setup.families.clone(),
                name: None,
            };
            let Ok(Some(reservation)) = ctx.hub.next(&free) else {
                return;
            };
            let Some(index) = idle.iter().position(|m| m.session == reservation.session) else {
                return;
            };
            let worker = idle.remove(index);
            trees -= 1;
            Self::queue_handoff(ctx, state, &reservation, &worker.app);
        }
    }

    fn queue_handoff(ctx: &Context, state: &mut State, reservation: &Reservation, app: &str) {
        let session = &reservation.session;
        let ticket = &reservation.ticket;
        let expires_at = reservation.expires_at;
        let code = Some(reservation.code.clone());
        // The model comes from the ticket's labels. Until they can be read the worker stays
        // reserved and nothing is sent, so a ticket never runs on a model it does not name.
        let Ok(detail) = ctx.hub.ticket(ticket) else {
            let placeholder = Reserved {
                ticket: ticket.clone(),
                expires_at,
                code,
                model: None,
                effort: String::new(),
                app: app.to_owned(),
            };
            state.reserved.insert(session.clone(), placeholder);
            state.unread.push((reservation.clone(), app.to_owned()));
            return;
        };
        let labels = detail.ticket.labels;
        let reserved = Reserved {
            ticket: ticket.clone(),
            expires_at,
            code,
            model: model_for(&labels, app),
            effort: label(&labels, "effort:").unwrap_or("high").to_owned(),
            app: app.to_owned(),
        };
        Self::queue_handoff_reserved(state, session, reserved);
    }

    fn queue_handoff_reserved(state: &mut State, session: &SessionId, reserved: Reserved) {
        state.reserved.insert(session.clone(), reserved.clone());
        let id = Self::next_id(state);
        state.pending.push_back(Queued {
            body: DispatchInstruction {
                id,
                action: DispatchAction::Handoff,
                session: Some(session.clone()),
                clear: true,
                model: reserved.model.clone(),
                effort: Some(reserved.effort.clone()),
                prompt: Some(format!(
                    "Run `ignite start {}{}` and follow what it prints.",
                    reserved.ticket,
                    reserved
                        .code
                        .as_ref()
                        .map(|code| format!(" --reservation {code}"))
                        .unwrap_or_default(),
                )),
            },
            purpose: Purpose::Handoff(reserved),
            gives: 0,
        });
    }

    fn queue_check(state: &mut State, session: &SessionId) {
        let prompt = format!(
            "Look up the session {session}: is it running, idle or gone? A lookup that failed is \
             `unknown`, never `gone`."
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

    fn ticket_changed(
        &self,
        ctx: &Context,
        state: &mut State,
        members: &[Member],
        ticket: &TicketId,
    ) {
        if let Some(holder) = state
            .held
            .iter()
            .find(|(_, t)| *t == ticket)
            .map(|(s, _)| s.clone())
        {
            if let Ok(detail) = ctx.hub.ticket(ticket) {
                self.deliver_tell(state, &holder, &detail);
            }
            return;
        }
        let now = ctx.clock.now();
        let culprits: BTreeMap<TicketId, SessionId> = Self::idle_workers(state, members, now)
            .into_iter()
            .filter_map(|m| {
                state
                    .last_ticket
                    .get(&m.session)
                    .map(|t| (t.clone(), m.session.clone()))
            })
            .collect();
        if culprits.is_empty() || !state.seen.insert(ticket.clone()) {
            return;
        }
        if let Ok(detail) = ctx.hub.ticket(ticket) {
            Self::offer_regression(state, &culprits, &detail, now);
        }
    }

    fn deliver_tell(&self, state: &mut State, holder: &SessionId, detail: &TicketDetail) {
        feed::deliver_tell(self, state, holder, detail);
    }

    fn offer_regression(
        state: &mut State,
        culprits: &BTreeMap<TicketId, SessionId>,
        detail: &TicketDetail,
        now: Timestamp,
    ) {
        let ticket = &detail.ticket;
        if ticket.status != Status::Open || !ticket.labels.iter().any(|l| l == "regression") {
            return;
        }
        let Some(session) = feed::caused_by(&ticket.body).and_then(|c| culprits.get(&c)) else {
            return;
        };
        let session = session.clone();
        let text = format!(
            "{} is a regression caused by your last ticket: {}. Run `ignite start {}` and fix it.",
            ticket.id, ticket.title, ticket.id
        );
        state.reserved.insert(
            session.clone(),
            Reserved {
                ticket: ticket.id.clone(),
                expires_at: now.plus_seconds(600.0),
                code: None,
                model: None,
                effort: "high".to_owned(),
                app: String::new(),
            },
        );
        Self::queue(
            state,
            Some(&session),
            DispatchAction::Message,
            &text,
            Purpose::Message,
        );
    }
}

#[cfg(test)]
#[path = "dispatchd_tests.rs"]
mod tests;
