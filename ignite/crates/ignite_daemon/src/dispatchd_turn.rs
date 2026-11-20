//! `ignite turn-ended`: what a pool worker's app reports when the worker ends its turn.
//!
//! A worker that ends its turn still holding its ticket leaves the ticket leased until the stall
//! rule (see `dispatchd_stall`) notices, about 45 minutes later. The Claude desktop app runs a
//! `Stop` hook at the end of every turn, and the hook asks this verb what to do. The verb
//! decides and the hook only relays:
//!
//! - `pass`: the session may stop. It is not a worker of this machine's pool (a person's own
//!   session, even one running `/ignite`, ends turns to talk to the person), it holds no ticket,
//!   it holds a justiciar ticket, or a `land`, `release` or `close` of the session is still
//!   running. Those verbs end the ticket, so a `land` waiting in the background holds the work
//!   legitimately.
//! - `pass`, too, when anything the session started is still running, since the worker is
//!   waiting on it. That is either an `ignite` verb of the session, such as a background
//!   `ignite test` or `ignite run -- <command>`, whose wake-up the daemon guarantees (see
//!   `wake`), or a live process under the session's own process (`--parent`, the hook's parent),
//!   such as a background shell command or a monitor script, which the agent app runs as its
//!   children and wakes the worker for when they exit. The hook runs under the same process, so
//!   the branch that holds the hook's own client (the request's PID) is left out. If the app
//!   never wakes such a worker, the stall rule still notices its silence. Before this, a worker
//!   that ended its turn on a long background run was released at its second stop, the release
//!   reset its tree and killed the run, and the woken worker claimed the ticket again beside the
//!   worker it was handed to.
//! - `block`: the first stop with the ticket held and nothing running. The text names the
//!   ticket and every way out (`ignite land`, `ignite release`, and `ignite justiciar` for a
//!   ticket that cannot be done as written), and the hook keeps the worker going with it. With
//!   nothing running the worker has really stopped, and Ignite could release at once; the one
//!   block is kept because it is the last chance to land finished work or to write a release
//!   note that says where the work stands, which the next worker needs.
//! - `released`: the worker stopped again (`--again`, from the hook's `stop_hook_active`) with
//!   nothing running and without landing or releasing, so Ignite gives the ticket back as the
//!   stall rule would. The tree is saved as the ticket's branch and the worker's last message
//!   (`--note`) is the release note. Without `--parent`, or when the processes cannot be read,
//!   only the session's `ignite` verbs count as running.
//! - `released` at the first stop, too, when the session's title starts with ⛔. Workers title
//!   themselves ⛔ when they stop unfinished, so a pool worker with that title that still holds
//!   its ticket is saying it is stuck: one once kept a ticket, its tree and a running build for
//!   hours under a ⛔ title. The ticket is released as for a second stop (releasing resets the
//!   tree, which stops the session's running commands, as `ignite release` does), and a
//!   justiciar is sent for, as `ignite justiciar` does, with the worker's last message as the
//!   reason. A ticket labelled `needs:person` is left alone: it waits on the person by design.
//!   The title comes from the desktop app's metadata (see `session_title`); a title that cannot
//!   be read changes nothing.
//!
//! A session released either way may not claim a ticket again until the dispatcher hands it one
//! (see `wake`), and the reply tells it to stop.
//!
//! The client sends this verb without the asking session (see `client::UNRECORDED_VERBS` in the
//! CLI), and the session comes from `--session` instead. A request with a session would replace
//! the session's current command with this one and end it, and a background `land` would then
//! look finished to both this verb and the stall rule.

use std::sync::Arc;

use ignite_core::ids::{SessionId, TicketId};
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::protocol::hub::{JusticiarReport, WorkerRole};
use ignite_core::traits::{Error, Result};

use super::Dispatcher;
use crate::server::{Context, Verb};

/// The label that marks a justiciar's ticket, whose session is never blocked or released.
const JUSTICIAR_LABEL: &str = "justiciar";

/// The label of a ticket that waits on the person, which a ⛔ title does not release.
const NEEDS_PERSON_LABEL: &str = "needs:person";

/// How a worker's title starts when it stops unfinished.
const STOPPED_MARK: char = '⛔';

/// The longest part of the worker's last message quoted in the justiciar report.
pub(super) const REASON_CHARS: usize = 400;

/// The longest part of the worker's last message kept in the release note.
pub(super) const NOTE_CHARS: usize = 2000;

/// `ignite turn-ended --session <id> [--again] [--note <last message>] [--parent <pid>]`.
pub struct TurnEndedVerb {
    dispatcher: Arc<Dispatcher>,
    /// The machine's hostname, which names it in a justiciar report.
    host: String,
}

impl TurnEndedVerb {
    pub fn new(dispatcher: Arc<Dispatcher>, host: String) -> TurnEndedVerb {
        TurnEndedVerb { dispatcher, host }
    }
}

/// The parsed arguments.
#[derive(Debug, Default)]
struct Ended {
    session: Option<SessionId>,
    again: bool,
    note: Option<String>,
    /// The session's own process, under which its background commands run.
    parent: Option<u32>,
}

fn parse(args: &[String]) -> Result<Ended> {
    let usage = || {
        Error::Invalid(
            "usage: ignite turn-ended --session <id> [--again] [--note <text>] [--parent <pid>]"
                .to_owned(),
        )
    };
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
            "--parent" => {
                let value = args.next().ok_or_else(usage)?;
                ended.parent = Some(value.parse().map_err(|_| usage())?);
            }
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
            Ok(ended) => self.dispatcher.turn_ended(ctx, &ended, req.pid, &self.host),
            Err(error) => Reply::text(Exit::Usage, error.to_string()),
        }
    }
}

/// The text that keeps a worker going after its first stop with `ticket` held.
fn block_text(ticket: &TicketId) -> String {
    format!(
        "block    You are ending your turn while you hold {ticket}, and nothing will report on \
         it. Before you stop, do one of these:\n\
         1. If the work is done, run `ignite land -m <note>`.\n\
         2. If it is not, run `ignite release -m <exact state>`, with the dependency when it \
         waits on another ticket (see `ignite guide release`).\n\
         3. If the ticket cannot be completed as written (its acceptance contradicts its body, \
         or gates on something you cannot control, such as a timing on a loaded machine), run \
         `ignite justiciar \"<what is impossible, and the evidence>\"` and then `ignite \
         release`.\n\
         To wait on a long command, start it in the background (heavy work as `ignite run -- \
         <command>`) and end your turn: while anything you started still runs, Ignite lets you \
         stop and you are woken when it ends. A turn that ends with nothing running is final: \
         only a final turn takes a ✅, ⛔ or 🔍 title, and a turn that ends to wait keeps ⚒️ (or \
         🚒), since a ⛔ title releases the ticket at once (see `ignite guide turn-end`).\n\
         A figure measured on a loaded machine is reported, never gated on or chased, unless \
         the acceptance names a deterministic measure.\n\
         If you stop again with nothing running and none of these done, Ignite releases {ticket} \
         itself, saves its tree as ignite/{ticket} and uses your last message as the note."
    )
}

/// The reason given with a release after the second stop, quoting the worker's last message.
pub(super) fn release_reason(note: Option<&str>) -> String {
    quoting(
        "The worker ended its turn twice holding the ticket without landing or releasing it.",
        note,
    )
}

/// The reason given with a release when the worker ended its turn titled ⛔.
pub(super) fn stopped_reason(note: Option<&str>) -> String {
    quoting(
        "The worker ended its turn titled ⛔ (stopped unfinished) while holding the ticket.",
        note,
    )
}

/// `head`, followed by the first [`NOTE_CHARS`] characters of `note` when there is one.
fn quoting(head: &str, note: Option<&str>) -> String {
    match note.map(str::trim).filter(|n| !n.is_empty()) {
        Some(note) => format!("{head} Its last message: {}", cut(note, NOTE_CHARS)),
        None => head.to_owned(),
    }
}

/// The first `chars` characters of `text`, with `…` when anything was cut.
fn cut(text: &str, chars: usize) -> String {
    let mut kept: String = text.chars().take(chars).collect();
    if kept.len() < text.len() {
        kept.push('…');
    }
    kept
}

/// The reason in the justiciar report sent for a worker that stopped unfinished holding
/// `ticket`: "<ticket>: worker stopped unfinished: <its last message, cut>".
pub(super) fn justiciar_reason(ticket: &TicketId, note: Option<&str>) -> String {
    let head = format!("{ticket}: worker stopped unfinished");
    match note.map(str::trim).filter(|n| !n.is_empty()) {
        Some(note) => format!("{head}: {}", cut(note, REASON_CHARS)),
        None => head,
    }
}

impl Dispatcher {
    fn turn_ended(&self, ctx: &Context, ended: &Ended, asker: Option<u32>, host: &str) -> Reply {
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
        if self.host.ending_ticket(session) {
            return pass("a land, release or close of the session is still running");
        }
        // A lookup that fails proves nothing, so the session is let go rather than blocked.
        let needs_person = match ctx.hub.ticket(&ticket) {
            Ok(detail) if detail.ticket.labels.iter().any(|l| l == JUSTICIAR_LABEL) => {
                return pass("a justiciar's session");
            }
            Ok(detail) => detail.ticket.labels.iter().any(|l| l == NEEDS_PERSON_LABEL),
            Err(error) => return pass(&format!("could not read {ticket}: {error}")),
        };
        let stopped = !needs_person
            && self
                .host
                .title(session)
                .is_some_and(|title| title.starts_with(STOPPED_MARK));
        if stopped {
            let note = ended.note.as_deref();
            let released = self.release_ended(session, &ticket, &stopped_reason(note));
            let sent = send_for_justiciar(ctx, host, session, &ticket, note);
            return Reply::text(Exit::Success, format!("{released}\n{sent}"));
        }
        let waiting_on = self.running_verb(session);
        self.note_turn_end(session, waiting_on.is_some(), ctx.clock.now());
        if let Some(verb) = waiting_on {
            return pass(&format!(
                "`ignite {verb}` is still running; Ignite wakes the session when it finishes"
            ));
        }
        let running = ended
            .parent
            .and_then(|parent| self.host.running_under(parent, asker))
            .unwrap_or_default();
        if !running.is_empty() {
            let pids: Vec<String> = running.iter().map(u32::to_string).collect();
            return pass(&format!(
                "the session's processes {} are still running; the app wakes it when they end",
                pids.join(", ")
            ));
        }
        if !ended.again {
            return Reply::text(Exit::Success, block_text(&ticket));
        }
        let released = self.release_ended(session, &ticket, &release_reason(ended.note.as_deref()));
        Reply::text(Exit::Success, released)
    }

    /// Releases `ticket` from `session` and says what happened, as the reply's first line.
    fn release_ended(&self, session: &SessionId, ticket: &TicketId, reason: &str) -> String {
        match self.host.release(session, ticket, reason) {
            Ok(()) => {
                self.note_released(session, ticket);
                format!(
                    "released {ticket} from {session}; its work is saved as ignite/{ticket}. \
                     Stop now: do not claim it or any other ticket and do not restart its \
                     commands; the dispatcher hands you your next ticket."
                )
            }
            Err(error) => format!("pass     could not release {ticket} from {session}: {error}"),
        }
    }
}

/// Sends for a justiciar for `ticket`, which `session` stopped working, as `ignite justiciar`
/// does. The hub folds reports with the same subject into one ticket, so a worker that ends
/// several turns titled ⛔ files one. Returns the reply's line saying what happened.
fn send_for_justiciar(
    ctx: &Context,
    host: &str,
    session: &SessionId,
    ticket: &TicketId,
    note: Option<&str>,
) -> String {
    let reason = justiciar_reason(ticket, note);
    let report = JusticiarReport {
        subject: format!("stopped:{ticket}"),
        title: format!("Justiciar asked for: {ticket}: worker stopped unfinished"),
        details: format!("Asked for {session}, which ended its turn titled ⛔:\n\n{reason}"),
        evidence: ctx.clock.now().to_string(),
        machine: ctx.machine.clone(),
        host: host.to_owned(),
        on_machine: false,
    };
    match ctx.hub.justiciar(&report) {
        Ok(filed) => format!("filed    {} (justiciar) for {ticket}", filed.ticket),
        Err(error) => format!("failed   no justiciar sent for {ticket}: {error}"),
    }
}
