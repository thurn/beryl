//! `ignite ticket`: the ticket verbs, which talk to the hub directly so they work while the
//! daemon is down. A port of the `tk` client: the same requests and the same JSON, with the
//! new `--allocation`, `--max-workers` and `--cores` flags on `create` and `update`.
//!
//! Every write is one `POST /batch`, so it succeeds or fails as a whole. Text output is one line
//! per ticket; `--json` prints the hub's reply with each ticket ID spelled with its root letter.

pub mod attach;
mod build;
pub mod http;
pub(crate) mod ids;
mod related;
mod render;
mod wait;

use std::io::Write;

use ignite_core::protocol::client::{Exit, Reply};
use serde_json::{Value, json};

use crate::args::{DepCommand, TicketCommand};
use crate::client::session_from_env;
pub(crate) use http::{Failure, Hub};
use http::{Result, usage};

/// The metadata key under which `create` saves the title of the Claude desktop session filing
/// the ticket, which the dashboard shows as the ticket's creator.
const CREATED_BY_TITLE: &str = "created_by_title";

pub fn run(command: &TicketCommand) -> Reply {
    let env = |name: &str| std::env::var(name).ok();
    let actor = session_from_env(env).map_or_else(|| "ignite".to_owned(), |s| s.to_string());
    let cwd = std::env::current_dir().unwrap_or_default();
    Hub::load(&cwd, actor, env)
        .and_then(|hub| execute(&hub, command))
        .unwrap_or_else(Failure::into_reply)
}

/// A reply carrying `value` as JSON and `text` as its human form.
fn reply(mut value: Value, text: impl FnOnce(&Value) -> String) -> Reply {
    ids::display(&mut value);
    Reply {
        exit: Exit::Success,
        text: text(&value),
        json: Some(value),
        handle: None,
    }
}

/// Like [`reply`], without the long text fields.
fn brief_reply(mut value: Value, text: impl FnOnce(&Value) -> String) -> Reply {
    ids::brief(&mut value);
    reply(value, text)
}

fn read(hub: &Hub, path: &[&str], query: &[(&str, String)]) -> Result<Value> {
    hub.get(path, query)?.json()
}

fn flag(query: &mut Vec<(&'static str, String)>, name: &'static str, on: bool) {
    if on {
        query.push((name, "1".to_owned()));
    }
}

/// Sends one or more operations as a batch and describes what they did.
/// `ticket dep add` and `ticket dep remove`.
fn dependency(hub: &Hub, command: &DepCommand) -> Result<Reply> {
    match command {
        DepCommand::Add {
            id,
            on,
            kind,
            external,
        } => write(
            hub,
            "linked",
            build::single(build::dep_add(id, on, kind.as_deref(), *external)),
        ),
        DepCommand::Remove { id, on } => write(
            hub,
            "unlinked",
            build::single(build::edge("dep_remove", id, on, None)),
        ),
        DepCommand::List { .. } => unreachable!("a read verb reached the write arms"),
    }
}

fn write(hub: &Hub, verb: &str, mut body: Value) -> Result<Reply> {
    let safe = build::prepare_batch(&mut body, build::random_key)?;
    if let Some(title) = &hub.actor_title {
        // The hub saves it as the session's title; a hub that predates titles ignores the key.
        body["actor_title"] = json!(title);
    }
    let answer = hub.post(&["batch"], &body, safe)?.json()?;
    Ok(reply(answer, |answer| render::written(verb, answer)))
}

pub fn execute(hub: &Hub, command: &TicketCommand) -> Result<Reply> {
    if let Some(reply) = lookup(hub, command)? {
        return Ok(reply);
    }
    match command {
        TicketCommand::Create {
            title,
            fields,
            labels,
            deps,
            independent,
            release,
            pending,
            automatic,
            attach,
        } => {
            let status = build::filing_status(*release, *pending);
            let mut op = build::create(title, fields, labels, deps, status)?;
            if let Some(filer) = &hub.actor_title {
                op["fields"]["metadata"] = json!({ CREATED_BY_TITLE: filer });
            }
            let created = write(
                hub,
                "created",
                build::filed(build::single(op), *automatic, *independent),
            )?;
            attach::after_create(hub, created, attach)
        }
        TicketCommand::Attach(args) => attach::attach(hub, args),
        TicketCommand::Update {
            id,
            fields,
            title,
            status,
            assignee,
            add_label,
            remove_label,
            release,
        } => {
            let changes = build::Changes {
                title: title.as_deref(),
                status: if *release {
                    Some(build::RELEASED)
                } else {
                    status.as_deref()
                },
                assignee: assignee.as_deref(),
                add_labels: add_label,
                remove_labels: remove_label,
            };
            // The hub releases the pending descendants of a ticket this update releases.
            let op = build::update(id, fields, &changes)?;
            write(hub, &build::update_verb(id, fields), build::single(op))
        }
        TicketCommand::Comment { id, text } => write(
            hub,
            "commented on",
            build::single(json!({"op": "comment", "id": id, "text": text})),
        ),
        TicketCommand::Claim { id } => write(
            hub,
            "claimed",
            build::single(json!({"op": "claim", "id": id})),
        ),
        TicketCommand::Close { id, reason } => write(
            hub,
            "closed",
            build::single(json!({"op": "close", "id": id, "reason": reason})),
        ),
        TicketCommand::Reopen { id } => write(
            hub,
            "reopened",
            build::single(json!({"op": "reopen", "id": id})),
        ),
        TicketCommand::Defer { id, until } => write(
            hub,
            "deferred",
            build::single(json!({"op": "defer", "id": id, "until": until})),
        ),
        TicketCommand::Dep { command } => dependency(hub, command),
        TicketCommand::Batch { file } => write(hub, "batch applied to", build::batch_file(file)?),
        TicketCommand::Wait {
            id,
            status,
            timeout,
        } => wait::wait(hub, id, status, *timeout),
        TicketCommand::Export => export(hub),
        // Answered by `lookup`.
        _ => unreachable!("a read verb reached the write arms"),
    }
}

/// The verbs that only read, or `None` for the ones that write.
fn lookup(hub: &Hub, command: &TicketCommand) -> Result<Option<Reply>> {
    Ok(Some(match command {
        TicketCommand::Show {
            ids,
            comments,
            history,
        } => related::show(hub, ids, *comments, *history)?,
        TicketCommand::Dep {
            command: DepCommand::List { id },
        } => related::dependencies(hub, id)?,
        TicketCommand::List {
            status,
            parent,
            ancestor,
            label,
            limit,
            all,
        } => {
            let mut query = vec![];
            for (name, value) in [
                ("status", status.clone()),
                ("parent", parent.clone()),
                ("ancestor", ancestor.clone()),
                ("limit", limit.filter(|&n| n != 0).map(|n| n.to_string())),
            ] {
                query.extend(value.map(|value| (name, value)));
            }
            query.extend(label.iter().map(|label| ("label", label.clone())));
            flag(&mut query, "all", *all);
            brief_reply(read(hub, &["tickets"], &query)?, render::list)
        }
        TicketCommand::Ready {
            ancestor,
            limit,
            brief: _,
        } => {
            let mut query = vec![("ready", "1".to_owned())];
            query.extend(ancestor.clone().map(|ancestor| ("ancestor", ancestor)));
            query.extend(limit.filter(|&n| n != 0).map(|n| ("limit", n.to_string())));
            brief_reply(read(hub, &["tickets"], &query)?, render::list)
        }
        TicketCommand::Children { id, open: true } => related::open_children(hub, id)?,
        TicketCommand::Children { id, open: false } => {
            let query = [("parent", id.clone()), ("all", "1".to_owned())];
            brief_reply(read(hub, &["tickets"], &query)?, render::list)
        }
        TicketCommand::Search {
            words,
            status,
            limit,
        } => {
            let mut query = vec![("search", words.join(" "))];
            query.extend(status.clone().map(|status| ("status", status)));
            query.extend(limit.filter(|&n| n != 0).map(|n| ("limit", n.to_string())));
            brief_reply(read(hub, &["tickets"], &query)?, render::list)
        }
        TicketCommand::Attachment { command } => attach::attachment(hub, command)?,
        TicketCommand::Graph { all } => {
            let mut query = vec![];
            flag(&mut query, "all", *all);
            reply(read(hub, &["graph"], &query)?, render::graph)
        }
        _ => return Ok(None),
    }))
}

/// Prints every ticket as one JSON line, a page at a time, so a large export is never held
/// whole.
fn export(hub: &Hub) -> Result<Reply> {
    let mut out = std::io::stdout().lock();
    let mut cursor = String::new();
    loop {
        let query = if cursor.is_empty() {
            vec![]
        } else {
            vec![("after", cursor.clone())]
        };
        let page = hub.get(&["export"], &query)?.body;
        let Some(last) = page.lines().rev().find(|line| !line.trim().is_empty()) else {
            return Ok(Reply::text(Exit::Success, ""));
        };
        let last: Value = serde_json::from_str(last).map_err(usage)?;
        let id = last["id"]
            .as_str()
            .or_else(|| last["ticket"]["id"].as_str())
            .ok_or_else(|| usage("an export row has no ticket ID"))?;
        if id <= cursor.as_str() {
            return Err(usage("export cursor did not advance"));
        }
        id.clone_into(&mut cursor);
        for line in page.lines().filter(|line| !line.trim().is_empty()) {
            let mut row: Value = serde_json::from_str(line).map_err(usage)?;
            ids::display(&mut row);
            writeln!(out, "{row}").map_err(usage)?;
        }
    }
}

/// Parses `ignite ticket ...` as the real command line does, for the tests.
#[cfg(test)]
fn parse(line: &str) -> TicketCommand {
    use crate::args::Cli;
    use clap::Parser;
    match Cli::parse_from(line.split_whitespace()).command {
        crate::args::Command::Ticket { command } => *command,
        _ => panic!("not a ticket command"),
    }
}

#[cfg(test)]
mod tests;
