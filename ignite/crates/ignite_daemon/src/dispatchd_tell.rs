//! `ignite tell`: a correction to work in flight. The text becomes a comment on the ticket, and
//! the ticket's metadata key `tell` is set to that comment's number in the same batch. The
//! daemon of the machine where the ticket is held sees the new number in the feed and sends the
//! text to the session working it (see `deliver_tell`). A ticket nobody holds gets only the
//! comment, which whoever starts it reads in the brief.
//!
//! An ordinary tell reaches the session when its turn ends. For a CLI worker (a session the
//! daemon runs as headless CLI turns, `cli-<app>-<id>`) a tell can also be urgent: `--now`, and
//! every tell from the session that holds an in-progress justiciar ticket, sets the metadata key
//! `tell_now` to the same comment number, and the holder's daemon then ends the running turn and
//! starts the next one with the text. A desktop session's turn cannot be ended from outside, so
//! `--now` to one is refused.

use std::collections::BTreeMap;

use ignite_core::ids::{SessionId, TicketId};
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::protocol::tickets::{
    BatchRequest, Operation, Precondition, Status, Ticket, TicketQuery,
};
use ignite_core::traits::{Error, Result};

use super::cli::is_cli_session;
use super::hub_exit;
use super::turn::JUSTICIAR_LABEL;
use crate::server::{Context, Verb};

/// Attempts at a batch whose tickets changed between reading and writing.
const ATTEMPTS: u32 = 3;

/// The usage line of `ignite tell`.
const USAGE: &str = "usage: ignite tell [--now] <ticket | session> \"<text>\" | \
                     ignite tell --all \"<text>\"";

/// `ignite tell [--now] <ticket | session> "<text>"` and `ignite tell --all "<text>"`.
pub struct TellVerb;

impl Verb for TellVerb {
    fn name(&self) -> &'static str {
        "tell"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        let flag = |name: &str| req.args.iter().any(|a| a == name);
        let (all, now) = (flag("--all"), flag("--now"));
        let words: Vec<&str> = req
            .args
            .iter()
            .map(String::as_str)
            .filter(|a| *a != "--all" && *a != "--now")
            .collect();
        let result = match (all, now, words.as_slice()) {
            (true, true, _) => Ok(Reply::text(
                Exit::Usage,
                "refused  `--now` works only for one ticket or session held by a CLI worker",
            )),
            (true, false, [text]) => tell_all(ctx, req, &signed(req, text)),
            (false, _, [target, text]) => tell_one(ctx, req, target, &signed(req, text), now),
            _ => Err(Error::Invalid(USAGE.to_owned())),
        };
        result.unwrap_or_else(|error| Reply::text(hub_exit(&error), error.to_string()))
    }
}

/// The text with the sending session in front, so a comment's author is a session a reader can
/// look up and not only the machine that wrote it. A caller with no session sends the text as
/// it is.
fn signed(req: &Request, text: &str) -> String {
    match &req.session {
        Some(session) => format!("from {session}: {text}"),
        None => text.to_owned(),
    }
}

/// Whether someone is working the ticket now, so the text should be sent as well as recorded.
fn held(ticket: &Ticket) -> bool {
    ticket.status == Status::InProgress && ticket.session.is_some()
}

/// Whether the tell is urgent for `ticket`'s holder: it is held by a CLI worker, and the sender
/// asked for `--now` or holds a justiciar ticket (`justiciar`).
fn urgent(ticket: &Ticket, now: bool, justiciar: bool) -> bool {
    held(ticket) && ticket.session.as_ref().is_some_and(is_cli_session) && (now || justiciar)
}

/// Whether the sending session holds an in-progress ticket labelled `justiciar`, among
/// `tickets`.
fn from_justiciar(req: &Request, tickets: &[Ticket]) -> bool {
    req.session.as_ref().is_some_and(|sender| {
        tickets.iter().any(|t| {
            t.session.as_ref() == Some(sender) && t.labels.iter().any(|l| l == JUSTICIAR_LABEL)
        })
    })
}

fn in_progress(ctx: &Context) -> Result<Vec<Ticket>> {
    ctx.hub.tickets(&TicketQuery {
        status: Some(Status::InProgress),
        ..TicketQuery::default()
    })
}

fn tell_one(ctx: &Context, req: &Request, target: &str, text: &str, now: bool) -> Result<Reply> {
    let tickets = in_progress(ctx)?;
    let ticket = if let Ok(id) = TicketId::new(target) {
        ctx.hub.ticket(&id)?.ticket
    } else {
        let session = SessionId::new(target)
            .map_err(|_| Error::Invalid(format!("{target} is neither a ticket nor a session")))?;
        tickets
            .iter()
            .find(|t| t.session.as_ref() == Some(&session))
            .cloned()
            .ok_or_else(|| Error::Invalid(format!("{session} holds no ticket")))?
    };
    let mark = held(&ticket);
    if let (true, true, Some(session)) = (now, mark, &ticket.session)
        && !is_cli_session(session)
    {
        return Ok(Reply::text(
            Exit::Usage,
            format!(
                "refused  `--now` works only for CLI workers, and {session} is a desktop session"
            ),
        ));
    }
    let urgent = urgent(&ticket, now, from_justiciar(req, &tickets));
    write(ctx, &[(ticket.id.clone(), mark, urgent)], text)?;
    let text = match (&ticket.session, mark) {
        (Some(session), true) if urgent => format!(
            "told     {} and its session {session}, whose running turn ends now",
            ticket.id
        ),
        (Some(session), true) => format!("told     {} and its session {session}", ticket.id),
        _ => format!(
            "commented {}; nobody holds it, so whoever starts it reads it in the brief",
            ticket.id
        ),
    };
    Ok(Reply::text(Exit::Success, text))
}

fn tell_all(ctx: &Context, req: &Request, text: &str) -> Result<Reply> {
    let in_progress = in_progress(ctx)?;
    let justiciar = from_justiciar(req, &in_progress);
    let tickets: Vec<(TicketId, bool, bool)> = in_progress
        .iter()
        .map(|t| (t.id.clone(), held(t), urgent(t, false, justiciar)))
        .collect();
    if tickets.is_empty() {
        return Ok(Reply::text(Exit::Success, "no ticket is in progress"));
    }
    write(ctx, &tickets, text)?;
    let names: Vec<&str> = tickets.iter().map(|(id, _, _)| id.as_str()).collect();
    Ok(Reply::text(
        Exit::Success,
        format!("told     {}", names.join(", ")),
    ))
}

/// Comments on every ticket, and marks each one `mark`ed for delivery, and each one marked
/// urgent as urgent too, in one batch. The comment's number is the next after the ticket's last,
/// which the batch's version preconditions keep true; a ticket that changed meanwhile is read
/// again.
fn write(ctx: &Context, tickets: &[(TicketId, bool, bool)], text: &str) -> Result<()> {
    let mut last = Error::Conflict("not attempted".to_owned());
    for _ in 0..ATTEMPTS {
        let mut request = BatchRequest::default();
        for (id, mark, urgent) in tickets {
            let detail = ctx.hub.ticket(id)?;
            request.preconditions.push(Precondition::Version {
                id: id.clone(),
                version: detail.ticket.version,
            });
            request.ops.push(Operation::Comment {
                id: id.to_string(),
                text: text.to_owned(),
            });
            if *mark {
                let n = detail
                    .comments
                    .iter()
                    .flatten()
                    .map(|c| c.n)
                    .max()
                    .unwrap_or(0)
                    + 1;
                request.ops.push(Operation::Update {
                    id: id.to_string(),
                    set: None,
                    add_labels: Vec::new(),
                    remove_labels: Vec::new(),
                    set_meta: tell_meta(n, *urgent),
                    unset_meta: Vec::new(),
                });
            }
        }
        match ctx.hub.batch(&request) {
            Ok(_) => return Ok(()),
            Err(error @ Error::Conflict(_)) => last = error,
            Err(error) => return Err(error),
        }
    }
    Err(last)
}

/// The metadata that marks comment `n` for delivery: `tell`, and for an urgent tell also
/// `tell_now` (see `deliver_tell`).
fn tell_meta(n: u32, urgent: bool) -> BTreeMap<String, serde_json::Value> {
    let mut meta = BTreeMap::from([("tell".to_owned(), serde_json::Value::from(n))]);
    if urgent {
        meta.insert(NOW_KEY.to_owned(), serde_json::Value::from(n));
    }
    meta
}

/// The metadata key that holds the number of the latest urgent tell's comment.
pub(super) const NOW_KEY: &str = "tell_now";
