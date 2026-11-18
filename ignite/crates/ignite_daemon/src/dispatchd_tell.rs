//! `ignite tell`: a correction to work in flight. The text becomes a comment on the ticket, and
//! the ticket's metadata key `tell` is set to that comment's number in the same batch. The
//! daemon of the machine where the ticket is held sees the new number in the feed and sends the
//! text to the session working it (see `deliver_tell`). A ticket nobody holds gets only the
//! comment, which whoever starts it reads in the brief.

use std::collections::BTreeMap;

use ignite_core::ids::{SessionId, TicketId};
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::protocol::tickets::{
    BatchRequest, Operation, Precondition, Status, Ticket, TicketQuery,
};
use ignite_core::traits::{Error, Result};

use super::hub_exit;
use crate::server::{Context, Verb};

/// Attempts at a batch whose tickets changed between reading and writing.
const ATTEMPTS: u32 = 3;

/// `ignite tell <ticket | session> "<text>"` and `ignite tell --all "<text>"`.
pub struct TellVerb;

impl Verb for TellVerb {
    fn name(&self) -> &'static str {
        "tell"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        let all = req.args.iter().any(|a| a == "--all");
        let words: Vec<&str> = req
            .args
            .iter()
            .map(String::as_str)
            .filter(|a| *a != "--all")
            .collect();
        let result = match (all, words.as_slice()) {
            (true, [text]) => tell_all(ctx, text),
            (false, [target, text]) => tell_one(ctx, target, text),
            _ => Err(Error::Invalid(
                "usage: ignite tell <ticket | session> \"<text>\" | ignite tell --all \"<text>\""
                    .to_owned(),
            )),
        };
        result.unwrap_or_else(|error| Reply::text(hub_exit(&error), error.to_string()))
    }
}

/// Whether someone is working the ticket now, so the text should be sent as well as recorded.
fn held(ticket: &Ticket) -> bool {
    ticket.status == Status::InProgress && ticket.session.is_some()
}

fn in_progress(ctx: &Context) -> Result<Vec<Ticket>> {
    ctx.hub.tickets(&TicketQuery {
        status: Some(Status::InProgress),
        ..TicketQuery::default()
    })
}

fn tell_one(ctx: &Context, target: &str, text: &str) -> Result<Reply> {
    let ticket = if let Ok(id) = TicketId::new(target) {
        ctx.hub.ticket(&id)?.ticket
    } else {
        let session = SessionId::new(target)
            .map_err(|_| Error::Invalid(format!("{target} is neither a ticket nor a session")))?;
        in_progress(ctx)?
            .into_iter()
            .find(|t| t.session.as_ref() == Some(&session))
            .ok_or_else(|| Error::Invalid(format!("{session} holds no ticket")))?
    };
    let mark = held(&ticket);
    write(ctx, &[(ticket.id.clone(), mark)], text)?;
    let text = match (&ticket.session, mark) {
        (Some(session), true) => format!("told     {} and its session {session}", ticket.id),
        _ => format!(
            "commented {}; nobody holds it, so whoever starts it reads it in the brief",
            ticket.id
        ),
    };
    Ok(Reply::text(Exit::Success, text))
}

fn tell_all(ctx: &Context, text: &str) -> Result<Reply> {
    let tickets: Vec<(TicketId, bool)> = in_progress(ctx)?
        .iter()
        .map(|t| (t.id.clone(), held(t)))
        .collect();
    if tickets.is_empty() {
        return Ok(Reply::text(Exit::Success, "no ticket is in progress"));
    }
    write(ctx, &tickets, text)?;
    let names: Vec<&str> = tickets.iter().map(|(id, _)| id.as_str()).collect();
    Ok(Reply::text(
        Exit::Success,
        format!("told     {}", names.join(", ")),
    ))
}

/// Comments on every ticket, and marks each one `mark`ed for delivery, in one batch. The
/// comment's number is the next after the ticket's last, which the batch's version
/// preconditions keep true; a ticket that changed meanwhile is read again.
fn write(ctx: &Context, tickets: &[(TicketId, bool)], text: &str) -> Result<()> {
    let mut last = Error::Conflict("not attempted".to_owned());
    for _ in 0..ATTEMPTS {
        let mut request = BatchRequest::default();
        for (id, mark) in tickets {
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
                    set_meta: BTreeMap::from([("tell".to_owned(), serde_json::Value::from(n))]),
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
