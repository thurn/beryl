//! `ignite ticket regression move`, and the note `ignite ticket close` prints about the
//! failing checks still on a regression ticket.
//!
//! The hub groups failing checks onto few regression tickets, so a ticket can hold a check that
//! another landing broke. Closing such a ticket while that check still fails gets it reopened
//! by the next failure, so a worker who closes it is told which checks are still on it, and
//! how to move one of them onto a ticket of its own.

use std::fmt::Write as _;

use ignite_core::protocol::client::{Exit, Reply};
use serde_json::{Value, json};

use super::http::{Hub, Result};
use super::ids;

/// Moves `check` onto `ticket` (`POST /regressions/move`). A repeat changes nothing, so the
/// request is safe to send again after an unclear failure.
pub fn move_check(hub: &Hub, check: &str, ticket: &str) -> Result<Reply> {
    let body = json!({"check": check, "ticket": ticket});
    let answer = hub.post(&["regressions", "move"], &body, true)?.json()?;
    let id = |key: &str| ids::spelled(answer[key].as_str().unwrap_or_default());
    let (from, to) = (id("from"), id("to"));
    let text = if from == to {
        format!("{check} is already on {to}")
    } else {
        format!("moved {check} from {from} to {to}; its later failures reopen {to}")
    };
    Ok(Reply {
        exit: Exit::Success,
        text,
        json: Some(answer),
        handle: None,
    })
}

/// Every check whose regression is not fixed, with its ticket as stored. Empty when the hub
/// cannot say, since the note is only advice and must not keep a close from happening.
pub fn unfixed(hub: &Hub) -> Vec<(String, String)> {
    let Ok(Ok(open)) = hub
        .get(&["regressions", "open"], &[])
        .map(|reply| reply.json())
    else {
        return Vec::new();
    };
    open["regressions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            Some((
                row["check"].as_str()?.to_owned(),
                row["ticket"].as_str()?.to_owned(),
            ))
        })
        .collect()
}

/// Adds to a close's text the checks in `unfixed` that were on the ticket it closed.
pub fn note_on_close(reply: &mut Reply, unfixed: &[(String, String)]) {
    let Some(closed) = reply
        .json
        .as_ref()
        .and_then(|answer| answer["results"][0]["id"].as_str())
        .map(str::to_owned)
    else {
        return;
    };
    let checks: Vec<&str> = unfixed
        .iter()
        .filter(|(_, ticket)| ids::spelled(ticket) == closed)
        .map(|(check, _)| check.as_str())
        .collect();
    if checks.is_empty() {
        return;
    }
    let _ = write!(
        reply.text,
        "\n{closed} still holds checks that have not passed since they failed: {}. The next \
         failure of any of them reopens {closed}. If one belongs to another ticket, run \
         `ignite ticket regression move <check> <ticket>`.",
        checks.join(", ")
    );
    if let Some(Value::Object(answer)) = reply.json.as_mut() {
        answer.insert("unfixed_checks".to_owned(), json!(checks));
    }
}
