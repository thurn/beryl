//! A Codex dispatcher's worker slots, and the pool as the hub's heartbeat and `ignite pool list`
//! show it.
//!
//! Codex keeps no standing worker sessions: each ticket runs in a new chat that the dispatcher
//! opens for it. So a Codex dispatcher registers a count of slots
//! (`ignite pool add <thread> --app codex --dispatcher --slots <n>`) instead of workers, and each
//! free slot counts as an idle Codex worker when the daemon asks the hub for tickets. A free slot
//! is offered to the hub under a placeholder session, `codex-slot-<k>`, so that the hub's
//! reservation names it as it names any other worker. The hub accepts a claim by the
//! reservation's code from this machine, whichever session sends it.
//!
//! The hand-off for a slot names no session. The dispatcher opens a new chat in the main
//! checkout, sends it the prompt and reports the chat's thread ID in its acknowledgement
//! (`ignite dispatch ack <id> ok <thread>`). The daemon then adds that chat to the pool as a
//! `single_use` worker whose parent is the dispatcher, and moves the reservation to it, so stall
//! checks, messages, tells and justiciar instructions reach it as they reach any other worker.
//!
//! The chat fills its slot until its ticket ends: the ticket lands or its lease expires (the chat
//! no longer holds a tree), its reservation lapses unclaimed, or the app says the chat is gone.
//! The chat then leaves the pool and the slot is free again. A chat is never handed a second
//! ticket.
//!
//! Slots are counted per dispatcher, not per machine, so a Claude pool and a Codex pool on one
//! machine stay independent. Both still share the machine's ticket trees and cores.
//!
//! An app in CLI mode (see `cli`) offers slots the same way with no dispatcher at all: `slots`
//! placeholders `cli-<app>-slot-<k>` from `[machine.<host>.pool.<app>]`, and one more,
//! `cli-<app>-slot-<slots + 1>`, that the hub is asked to fill only with a ticket that holds
//! `master` back. That extra slot replaces keeping the last idle worker for justiciar tickets.
//! A reservation for a slot starts a CLI session `cli-<app>-<id>`, which joins the pool as a
//! single-use worker and fills a slot until its ticket ends, as a Codex chat does. While the app
//! runs in CLI mode, its desktop workers and its Codex dispatcher's slots are offered nothing.

use ignite_core::ids::SessionId;
use ignite_core::protocol::hub::{WorkerRole, WorkerState};
use ignite_core::time::Timestamp;
use ignite_core::traits::Result;

use super::{Dispatcher, Purpose, Queued, State, cli};
use crate::pool::Member;

/// The start of every slot's placeholder session.
const SLOT_PREFIX: &str = "codex-slot-";

/// Whether `session` is a slot's placeholder rather than a real session.
pub(super) fn is_slot(session: &SessionId) -> bool {
    session.as_str().starts_with(SLOT_PREFIX)
}

/// The placeholder worker of every slot that no chat fills, reserved or not, for each Codex
/// dispatcher in `members`. A reserved placeholder is a hand-off on its way to a new chat; the
/// chats that already hold tickets fill the other placeholders first.
fn placeholders(state: &State, members: &[Member]) -> Vec<Member> {
    let mut slots = Vec::new();
    for dispatcher in members.iter().filter(|m| {
        m.role == WorkerRole::Dispatcher
            && m.slots > 0
            && !m.down
            && !cli::in_cli_mode(state, &m.app)
    }) {
        let chats = members
            .iter()
            .filter(|m| m.single_use && !m.down && m.parent.as_ref() == Some(&dispatcher.session))
            .count();
        let sessions: Vec<SessionId> = (1..=dispatcher.slots)
            .filter_map(|k| SessionId::new(format!("{SLOT_PREFIX}{k}")).ok())
            .collect();
        let reserved = sessions.iter().filter(|s| state.reserved.contains_key(*s));
        let free = sessions
            .iter()
            .filter(|s| !state.reserved.contains_key(*s))
            .skip(chats);
        slots.extend(reserved.chain(free).map(|session| Member {
            session: session.clone(),
            app: dispatcher.app.clone(),
            role: WorkerRole::Worker,
            down: false,
            models: dispatcher.models.clone(),
            parent: Some(dispatcher.session.clone()),
            files: Vec::new(),
            added_at: dispatcher.added_at,
            slots: 0,
            single_use: false,
        }));
    }
    slots.extend(cli_placeholders(state, members));
    slots
}

/// The placeholder of every CLI slot that no CLI session fills, reserved or not, for each app in
/// CLI mode. The slot for tickets that hold `master` back is offered only while fewer than all
/// of the app's slots are busy.
fn cli_placeholders(state: &State, members: &[Member]) -> Vec<Member> {
    let mut slots = Vec::new();
    for layout in &state.cli_layout {
        let sessions = members
            .iter()
            .filter(|m| {
                m.single_use && !m.down && m.app == layout.app && cli::is_cli_session(&m.session)
            })
            .count();
        let all: Vec<SessionId> = (1..=layout.slots + 1)
            .filter_map(|k| cli::placeholder(&layout.app, k))
            .collect();
        let Some((extra, ordinary)) = all.split_last() else {
            continue;
        };
        let reserved = |s: &SessionId| state.reserved.contains_key(s);
        let busy = sessions + all.iter().filter(|s| reserved(s)).count();
        let free = ordinary.iter().filter(|s| !reserved(s)).skip(sessions);
        let extra_offered = reserved(extra) || busy < all.len();
        let chosen = ordinary
            .iter()
            .filter(|s| reserved(s))
            .chain(free)
            .chain(extra_offered.then_some(extra));
        slots.extend(chosen.map(|session| Member {
            session: session.clone(),
            app: layout.app.clone(),
            role: WorkerRole::Worker,
            down: false,
            models: layout.models.clone(),
            parent: None,
            files: Vec::new(),
            added_at: Timestamp::from_millis(0),
            slots: 0,
            single_use: false,
        }));
    }
    slots
}

/// `members` and a placeholder worker for each slot no chat fills, for choosing a worker to give
/// a ticket to.
pub(super) fn with_slots(state: &State, members: &[Member]) -> Vec<Member> {
    let mut all = members.to_vec();
    all.extend(placeholders(state, members));
    all
}

/// The workers that can be handed tickets, idle or not: the standing workers that are not down,
/// every Codex dispatcher's slots, and every CLI slot that takes any ticket, leaving out the
/// workers of apps in CLI mode that are not CLI slots.
pub(super) fn capacity(state: &State, members: &[Member]) -> usize {
    let standing = members
        .iter()
        .filter(|m| m.role == WorkerRole::Worker && !m.down && !m.single_use)
        .filter(|m| !cli::in_cli_mode(state, &m.app))
        .count();
    let slots: u32 = members
        .iter()
        .filter(|m| m.role == WorkerRole::Dispatcher && !m.down)
        .filter(|m| !cli::in_cli_mode(state, &m.app))
        .map(|m| m.slots)
        .sum::<u32>()
        + state.cli_layout.iter().map(|l| l.slots).sum::<u32>();
    standing + usize::try_from(slots).unwrap_or(usize::MAX)
}

/// The session an instruction is for. A hand-off to a slot names no session, so it is the slot
/// whose reservation holds the hand-off's ticket.
pub(super) fn handoff_session(state: &State, queued: &Queued) -> Option<SessionId> {
    if let Some(session) = &queued.body.session {
        return Some(session.clone());
    }
    let Purpose::Handoff(handed) = &queued.purpose else {
        return None;
    };
    state
        .reserved
        .iter()
        .find(|(session, reserved)| is_slot(session) && reserved.ticket == handed.ticket)
        .map(|(session, _)| session.clone())
}

impl Dispatcher {
    /// Records the new chat the dispatcher opened for `slot`'s hand-off, from the thread ID that
    /// starts `reason`. Returns what to add to the acknowledgement.
    pub(super) fn slot_opened(
        &self,
        state: &mut State,
        members: &[Member],
        slot: &SessionId,
        reason: &str,
        now: Timestamp,
    ) -> String {
        let thread = reason
            .split_whitespace()
            .next()
            .and_then(|t| SessionId::new(t).ok());
        let (Some(thread), Some(reserved)) = (thread, state.reserved.get(slot).cloned()) else {
            return format!(
                "; no thread ID was reported (`ok <thread>`), so {slot} stays taken until its \
                 reservation lapses"
            );
        };
        let dispatcher = members
            .iter()
            .find(|m| m.role == WorkerRole::Dispatcher && m.app == reserved.app);
        let chat = Member {
            session: thread.clone(),
            app: reserved.app.clone(),
            role: WorkerRole::Worker,
            down: false,
            models: dispatcher.map(|d| d.models.clone()).unwrap_or_default(),
            parent: dispatcher.map(|d| d.session.clone()),
            files: Vec::new(),
            added_at: now,
            slots: 0,
            single_use: true,
        };
        if let Err(error) = self.pool.put(&chat) {
            return format!("; could not add {thread} to the pool: {error}");
        }
        state.reserved.remove(slot);
        let ticket = reserved.ticket.clone();
        state.reserved.insert(thread.clone(), reserved);
        format!("; {thread} takes {ticket} in {slot}")
    }

    /// Removes from the pool every chat whose ticket ended: it holds no tree and no reservation,
    /// or the app said it is gone. Its slot is then free. Returns whether any chat left.
    pub(super) fn end_chats(&self) -> bool {
        let Ok(members) = self.pool.members() else {
            return false;
        };
        let ended: Vec<SessionId> = {
            let state = self.state();
            members
                .iter()
                .filter(|m| m.single_use)
                .filter(|m| {
                    m.down
                        || (!state.held.contains_key(&m.session)
                            && !state.reserved.contains_key(&m.session))
                })
                .map(|m| m.session.clone())
                .collect()
        };
        let mut any = false;
        for session in ended {
            if self.pool.remove(&session).is_ok() {
                self.forget(&session);
                any = true;
            }
        }
        any
    }

    /// Every member as the hub's heartbeat and `ignite pool list` show it, and each slot no chat
    /// fills as an idle or reserved worker of its app.
    pub fn view(&self) -> Result<Vec<WorkerState>> {
        self.update_cli_layout();
        let members = self.pool.members()?;
        let holders = self.host.holders();
        let state = self.state();
        let slots = placeholders(&state, &members);
        Ok(members
            .iter()
            .chain(&slots)
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
}
