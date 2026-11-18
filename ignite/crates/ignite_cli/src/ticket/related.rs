//! The read verbs that gather several tickets: `dep list`, `show` with several IDs and
//! `children --open`. The hub returns one ticket's detail per request, so each of these asks for
//! the tickets it starts from and then for each ticket they name.
//!
//! Every value here keeps the hub's own IDs until the reply is built, where
//! [`super::reply`] spells them with their `T` once.

use serde_json::{Value, json};

use super::http::{Hub, Result};
use super::{brief_reply, flag, read, render, reply};
use ignite_core::protocol::client::Reply;

/// One ticket's detail, with its comments when `comments` is set.
fn detail(hub: &Hub, id: &str, comments: bool) -> Result<Value> {
    let mut query = vec![];
    flag(&mut query, "comments", comments);
    read(hub, &["tickets", id], &query)
}

/// `{id, status, title}` of a ticket, plus `kind` for the edge that led to it.
fn summary(ticket: &Value, kind: &str) -> Value {
    json!({
        "id": ticket["id"],
        "status": ticket["status"],
        "title": ticket["title"],
        "kind": kind,
    })
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("")
}

/// `ignite ticket dep list <id>`: what the ticket waits on and what waits on it.
pub fn dependencies(hub: &Hub, id: &str) -> Result<Reply> {
    let ticket = detail(hub, id, false)?;
    let own = text(&ticket, "id");
    let mut waits_on = vec![];
    let mut waited_on_by = vec![];
    for edge in ticket["edges"].as_array().into_iter().flatten() {
        let (src, dst, kind) = (text(edge, "src"), text(edge, "dst"), text(edge, "kind"));
        if src == own {
            waits_on.push(summary(&detail(hub, dst, false)?, kind));
        } else if dst == own {
            waited_on_by.push(summary(&detail(hub, src, false)?, kind));
        }
    }
    let value = json!({
        "id": ticket["id"],
        "status": ticket["status"],
        "title": ticket["title"],
        "waits_on": waits_on,
        "waited_on_by": waited_on_by,
    });
    Ok(reply(value, render::dependencies))
}

/// `ignite ticket show <id> <id>...`: one detail for one ID, as before; an array for several.
pub fn show(hub: &Hub, ids: &[String], comments: bool, history: bool) -> Result<Reply> {
    let mut query = vec![];
    flag(&mut query, "comments", comments);
    flag(&mut query, "history", history);
    let mut details = ids
        .iter()
        .map(|id| read(hub, &["tickets", id], &query))
        .collect::<Result<Vec<_>>>()?;
    if details.len() == 1 {
        return Ok(reply(details.remove(0), render::detail));
    }
    Ok(reply(Value::Array(details), |details| {
        details
            .as_array()
            .into_iter()
            .flatten()
            .map(render::detail)
            .collect::<Vec<_>>()
            .join("\n\n")
    }))
}

/// `ignite ticket children <id> --open`: each child that is not closed, with its assignee, what
/// blocks it and the first line of its last comment.
pub fn open_children(hub: &Hub, id: &str) -> Result<Reply> {
    let children = read(hub, &["tickets"], &[("parent", id.to_owned())])?;
    let mut rows = vec![];
    for child in children.as_array().into_iter().flatten() {
        let mut detail = detail(hub, text(child, "id"), true)?;
        let own = text(&detail, "id").to_owned();
        let last = detail["comments"]
            .as_array()
            .and_then(|comments| comments.last())
            .map(|comment| {
                text(comment, "text")
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_owned()
            });
        if let Some(fields) = detail.as_object_mut() {
            if let Some(Value::Array(blockers)) = fields.get_mut("blocked_by") {
                // A ticket that is itself under way is listed as its own blocker.
                blockers.retain(|blocker| text(blocker, "id") != own);
            }
            for key in ["edges", "children", "ready"] {
                fields.remove(key);
            }
            if let Some(last) = last {
                fields.insert("last_comment".to_owned(), Value::String(last));
            }
        }
        rows.push(detail);
    }
    Ok(brief_reply(Value::Array(rows), render::open_children))
}
