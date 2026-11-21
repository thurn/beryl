//! Delivers hub messages to CLI workers and builds prompts for regressions and reverted tickets.

use ignite_core::ids::{MachineId, SessionId, TicketId};
use ignite_core::protocol::tickets::{
    CULPRIT_AFFINITY_KEY, CulpritAffinity, REVERTED_KEY, Status, TicketDetail, regression_culprit,
};

use crate::clid::Resume;

use super::{Reserved, State, cli, start_arguments, tell};

pub(super) fn deliver_tell(state: &mut State, holder: &SessionId, detail: &TicketDetail) {
    if !cli::is_cli_session(holder) {
        return;
    }
    let ticket = &detail.ticket;
    let Some(tell) = ticket
        .metadata
        .get("tell")
        .and_then(serde_json::Value::as_u64)
    else {
        return;
    };
    if tell <= state.tells.get(&ticket.id).copied().unwrap_or(0) {
        return;
    }
    state.tells.insert(ticket.id.clone(), tell);
    let Some(comment) = detail
        .comments
        .iter()
        .flatten()
        .find(|c| u64::from(c.n) == tell)
    else {
        return;
    };
    let held_here =
        ticket.status == Status::InProgress && ticket.assignee.as_deref() == Some(holder.as_str());
    let claimed_before = ticket
        .claimed_at
        .is_some_and(|claimed| comment.at >= claimed);
    if !held_here || !claimed_before {
        return;
    }
    let text = format!("Message about {}: {}", ticket.id, comment.text);
    let urgent = ticket
        .metadata
        .get(tell::NOW_KEY)
        .and_then(serde_json::Value::as_u64)
        == Some(tell);
    state.cli_calls.push(cli::Call::Message {
        session: holder.clone(),
        text,
        urgent,
    });
}

/// The transcript a regression or reverted ticket should use, when available.
pub(super) fn handoff(
    detail: &TicketDetail,
    reserved: &Reserved,
    machine: &MachineId,
) -> Option<Resume> {
    let ticket = &detail.ticket;
    let reverted = ticket.metadata.contains_key(REVERTED_KEY);
    let culprit = if reverted {
        Some(ticket.id.clone())
    } else if ticket.labels.iter().any(|label| label == "regression") {
        regression_culprit(ticket)
    } else {
        return None;
    };
    let affinity = ticket
        .metadata
        .get(CULPRIT_AFFINITY_KEY)
        .and_then(|value| serde_json::from_value::<CulpritAffinity>(value.clone()).ok());
    let fresh_reason = affinity
        .filter(|pin| pin.landing.machine != *machine)
        .map(|pin| {
            format!(
                "the transcript is on {}",
                pin.machine_label.trim_start_matches("machine:")
            )
        });
    let prompt = match &culprit {
        Some(_) if reverted => reverted_prompt(detail, reserved),
        Some(culprit) => regression_prompt(reserved, &ticket.title, culprit),
        None => String::new(),
    };
    Some(Resume {
        culprit,
        prompt,
        fresh_reason,
    })
}

/// The hand-off prompt of `reserved`, a regression, for the worker whose landing caused it.
fn regression_prompt(reserved: &Reserved, title: &str, culprit: &TicketId) -> String {
    format!(
        "{} is a regression caused by your landing of {culprit}: {title}. Run `ignite start {}` and fix \
         it.",
        reserved.ticket,
        start_arguments(reserved)
    )
}

/// The hand-off prompt of `reserved`, a ticket whose landing a tip round reverted, for the
/// worker that landed it. It names the checks that did not build.
fn reverted_prompt(detail: &TicketDetail, reserved: &Reserved) -> String {
    let checks = detail
        .ticket
        .metadata
        .get(REVERTED_KEY)
        .and_then(|m| m.get("checks"))
        .and_then(serde_json::Value::as_array)
        .map(|checks| {
            checks
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    format!(
        "your landing of {} was reverted: the tip did not build with it ({checks}). Run `ignite \
         start {}`",
        reserved.ticket,
        start_arguments(reserved)
    )
}
