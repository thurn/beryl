//! `ignite turn-ended`: what a pool worker's app reports when the worker ends its turn.
//!
//! A worker that ends its turn still holding its ticket leaves the ticket leased until the stall
//! rule (see `dispatchd_stall`) notices, about 45 minutes later. The Claude desktop app runs a
//! `Stop` hook at the end of every turn, and the hook asks this verb what to do. The verb
//! decides and the hook only relays:
//!
//! - `pass`: the session may stop. It is not a worker of this machine's pool (a person's own
//!   session, even one running `/ignite`, ends turns to talk to the person), it holds no ticket,
//!   it holds a justiciar ticket, or one of its `ignite` commands is still running (a `land`
//!   waiting in the background holds the work legitimately).
//! - `block`: the first stop with the ticket held. The text names the ticket and the two ways
//!   out, `ignite land` and `ignite release`, and the hook keeps the worker going with it.
//! - `released`: the worker stopped again (`--again`, from the hook's `stop_hook_active`) without
//!   landing or releasing, so Ignite gives the ticket back as the stall rule would. The tree is
//!   saved as the ticket's branch and the worker's last message (`--note`) is the release note.
//!
//! The client sends this verb without the asking session (see `client::UNRECORDED_VERBS` in the
//! CLI), and the session comes from `--session` instead. A request with a session would replace
//! the session's current command with this one and end it, and a background `land` would then
//! look finished to both this verb and the stall rule.

use std::sync::Arc;

use ignite_core::ids::{SessionId, TicketId};
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::protocol::hub::WorkerRole;
use ignite_core::traits::{Error, Result};

use super::Dispatcher;
use crate::server::{Context, Verb};

/// The label that marks a justiciar's ticket, whose session is never blocked or released.
const JUSTICIAR_LABEL: &str = "justiciar";

/// The longest part of the worker's last message kept in the release note.
pub(super) const NOTE_CHARS: usize = 2000;

/// `ignite turn-ended --session <id> [--again] [--note <last message>]`.
pub struct TurnEndedVerb {
    dispatcher: Arc<Dispatcher>,
}

impl TurnEndedVerb {
    pub fn new(dispatcher: Arc<Dispatcher>) -> TurnEndedVerb {
        TurnEndedVerb { dispatcher }
    }
}

/// The parsed arguments.
#[derive(Debug, Default)]
struct Ended {
    session: Option<SessionId>,
    again: bool,
    note: Option<String>,
}

fn parse(args: &[String]) -> Result<Ended> {
    let usage =
        || Error::Invalid("usage: ignite turn-ended --session <id> [--again] [--note <text>]".to_owned());
    let mut ended = Ended::default();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--session" => {
                let value = args.next().ok_or_else(usage)?;
                ended.session = Some(SessionId::new(value).map_err(|_| usage())?);
            }
            "--again" => ended.again = true,
            "--note" => ended.note = Some(args.next().ok_or_else(usage)?.clone()),
            _ => return Err(usage()),
        }
    }
    if ended.session.is_none() {
        return Err(usage());
    }
    Ok(ended)
}

impl Verb for TurnEndedVerb {
    fn name(&self) -> &'static str {
        "turn-ended"
    }

    /// The hook runs it from wherever the session's shell is.
    fn needs_the_tree_lease(&self) -> bool {
        false
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        match parse(&req.args) {
            Ok(ended) => self.dispatcher.turn_ended(ctx, &ended),
            Err(error) => Reply::text(Exit::Usage, error.to_string()),
        }
    }
}

/// The text that keeps a worker going after its first stop with `ticket` held.
fn block_text(ticket: &TicketId) -> String {
    format!(
        "block    You are ending your turn while you hold {ticket}, and nothing will report on \
         it. Before you stop, run `ignite land -m <note>` if the work is done, or `ignite \
         release -m <exact state>` if it is not, with the dependency when it waits on another \
         ticket (see `ignite guide release`). If you stop again without either, Ignite releases \
         {ticket} itself, saves its tree as ignite/{ticket} and uses your last message as the \
         note."
    )
}

/// The reason given with a release after the second stop, quoting the worker's last message.
pub(super) fn release_reason(note: Option<&str>) -> String {
    let head = "The worker ended its turn twice holding the ticket without landing or releasing \
                it.";
    match note.map(str::trim).filter(|n| !n.is_empty()) {
        Some(note) => {
            let mut kept: String = note.chars().take(NOTE_CHARS).collect();
            if kept.len() < note.len() {
                kept.push('…');
            }
            format!("{head} Its last message: {kept}")
        }
        None => head.to_owned(),
    }
}

impl Dispatcher {
    fn turn_ended(&self, ctx: &Context, ended: &Ended) -> Reply {
        let Some(session) = &ended.session else {
            return Reply::text(Exit::Usage, "no session");
        };
        let pass = |why: &str| Reply::text(Exit::Success, format!("pass     {why}"));
        let worker = self
            .pool
            .member(session)
            .ok()
            .flatten()
            .is_some_and(|m| m.role == WorkerRole::Worker);
        if !worker {
            return pass("not a worker of this machine's pool");
        }
        let Some(ticket) = self.host.holders().get(session).cloned() else {
            return pass("holds no ticket");
        };
        if self.host.busy(session) {
            return pass("an ignite command of the session is still running");
        }
        // A lookup that fails proves nothing, so the session is let go rather than blocked.
        match ctx.hub.ticket(&ticket) {
            Ok(detail) if detail.ticket.labels.iter().any(|l| l == JUSTICIAR_LABEL) => {
                return pass("a justiciar's session");
            }
            Ok(_) => {}
            Err(error) => return pass(&format!("could not read {ticket}: {error}")),
        }
        if !ended.again {
            return Reply::text(Exit::Success, block_text(&ticket));
        }
        let reason = release_reason(ended.note.as_deref());
        match self.host.release(session, &ticket, &reason) {
            Ok(()) => Reply::text(
                Exit::Success,
                format!("released {ticket} from {session}; its work is saved as ignite/{ticket}"),
            ),
            Err(error) => Reply::text(
                Exit::Success,
                format!("pass     could not release {ticket} from {session}: {error}"),
            ),
        }
    }
}
