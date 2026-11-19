//! The replies of a landing that are built from its results alone: a failed gate's, and the
//! session title of a landed ticket.

use std::path::Path;

use serde_json::json;

use ignite_core::ids::TicketId;
use ignite_core::protocol::client::{Exit, Reply};

use super::{Rebased, line, short};
use crate::checks::GateRun;
use crate::checks::verbs::{built_lines, gate_lines, render};
use crate::server::Context;

/// The reply to a gate that failed: exit code 2, the failures with paths relative to the
/// repository, and a rerun command that works in the ticket tree.
pub(super) fn failed_gate(
    staging: &str,
    rebased: &Rebased,
    gated: &GateRun,
    gate_path: &Path,
) -> Reply {
    let mut lines = vec![line(
        "rebased",
        format!("onto {staging} {}", short(&rebased.onto)),
    )];
    lines.extend(built_lines(&gated.run));
    lines.extend(gate_lines(gated));
    let rerun = gated.run.failures.first().map_or_else(
        || "ignite check".to_owned(),
        |failure| format!("ignite test {}", failure.check),
    );
    lines.push(line("rerun", rerun));
    lines.push(line(
        "next",
        "fix it in your tree; nothing landed and the tree stays rebased",
    ));
    let prefix = format!("{}/", gate_path.display());
    let text = render(&lines).replace(&prefix, "");
    Reply {
        exit: Exit::CheckFailed,
        text,
        json: Some(json!({
            "failures": gated.run.failures.iter().map(|f| json!({
                "check": f.check, "outcome": f.outcome, "at": f.at, "message": f.message,
            })).collect::<Vec<_>>(),
        })),
        handle: None,
    }
}

/// The session title for a landed ticket: `✅ [<ticket>] <ticket title>`, the title that
/// `ignite start` printed with its working emoji. When the hub cannot be asked, only the ticket
/// ID follows the emoji.
pub(super) fn done_title(ctx: &Context, ticket: &TicketId) -> String {
    match ctx.hub.ticket(ticket) {
        Ok(detail) => {
            let title = detail.ticket.title.lines().next().unwrap_or("").trim();
            format!("✅ [{ticket}] {title}")
        }
        Err(_) => format!("✅ [{ticket}]"),
    }
}
