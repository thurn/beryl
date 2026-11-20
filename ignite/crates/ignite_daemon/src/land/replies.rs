//! The replies of a landing that are built from its results alone: a landed one's, a failed
//! gate's, and the session title of a landed ticket.

use std::path::Path;

use serde_json::json;

use ignite_core::ids::TicketId;
use ignite_core::protocol::client::{Exit, Reply};

use super::records::LandingRecord;
use super::{Landing, Rebased, Settled, Stop, Summary, line, short, stop};
use crate::checks::GateRun;
use crate::checks::verbs::{built_lines, gate_lines, render};
use crate::server::Context;

/// The `next` line of a refused landing. The gate has already compared the failures with the
/// tip, and `GateRun::base` records the outcome: when it says the tip could not be asked, the
/// worker reruns the check once and then releases; otherwise the failure is the change's.
fn refused_next(gated: &GateRun) -> String {
    let tip_unknown = gated
        .base
        .as_deref()
        .is_some_and(|base| base.starts_with("not compared with the tip"));
    let not_started = gated.run.build_failures.iter().any(|(_, output)| {
        std::fs::read_to_string(output)
            .is_ok_and(|text| text.starts_with(crate::build::NOT_STARTED))
    });
    if not_started {
        "a build could not start because the machine failed, not because of your change: \
         land again; if it repeats, `ignite release` with a note naming the build and its \
         record; see `ignite guide blocked`"
            .to_owned()
    } else if tip_unknown {
        "run the failing check once with `ignite test <check>`; if it passes, land again, \
         otherwise `ignite release` with a note naming the check, the tip commit and the first \
         lines of its output; see `ignite guide blocked`"
            .to_owned()
    } else {
        "the failure is yours, even if it looks unrelated: fix it in your tree and land again; \
         nothing landed and the tree stays rebased; see `ignite guide blocked`"
            .to_owned()
    }
}

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
    lines.push(line("next", refused_next(gated)));
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

/// The reply of a landing that stopped because someone asked it to (see `crate::stop`); the
/// server replaces it with the stopped reply.
#[allow(clippy::unnecessary_box_returns)]
pub(super) fn stopped() -> Stop {
    stop(
        Exit::Conflict,
        &[line("stopped", "the landing was stopped")],
    )
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

impl Landing {
    /// The reply of a landing that has pushed and settled.
    pub(super) fn reply(
        ctx: &Context,
        record: &LandingRecord,
        settled: Settled,
        summary: Option<&Summary>,
        note: Option<&str>,
    ) -> Reply {
        let Settled {
            closed,
            released,
            problems,
        } = settled;
        let ticket = &record.landed.ticket;
        let staging = &ctx.config.repository.staging;
        let mut lines = Vec::new();
        if let Some(summary) = summary {
            lines.push(line(
                "rebased",
                format!(
                    "onto {staging} {} ({} commits landed since start)",
                    short(&summary.tip),
                    summary.landed_since
                ),
            ));
            if let Some(formatted) = &summary.formatted {
                lines.extend(formatted.line());
                lines.extend(formatted.warnings());
            }
            let gate_s = if let Some(run) = &summary.express {
                lines.push(run.line());
                run.ran.iter().map(|(_, seconds)| seconds).sum()
            } else {
                lines.extend(built_lines(&summary.gated.run));
                lines.extend(gate_lines(&summary.gated));
                summary.gated.seconds
            };
            lines.extend(summary.timeline.lines(ctx.clock.now(), gate_s));
        }
        lines.push(line(
            "landed",
            format!(
                "{} on {staging}; {closed}; {released}",
                short(&record.commit)
            ),
        ));
        if let Some(release) = summary.and_then(|summary| summary.release.clone()) {
            lines.push(release);
        }
        lines.extend(problems);
        lines.push(line("title", done_title(ctx, ticket)));
        // A landing that moved the release names the wait for it to reach every machine.
        let (next, deploy) = crate::deploy::landed_next(
            ctx.config.repository.release.as_deref(),
            summary
                .and_then(|s| s.release.as_ref())
                .map(|(_, r)| r.as_str()),
            &record.commit,
        );
        // Checks left out as known-failing are not this ticket's to fix.
        let next = if summary.is_some_and(|s| !s.gated.known_failing.is_empty()) {
            format!(
                "{next}; fix none of the known-failing checks and take none of their \
                 regression tickets (`ignite guide blocked`)"
            )
        } else {
            next
        };
        lines.push(line("next", next));
        let budget = summary.and_then(|s| s.gated.budget.as_ref());
        Reply {
            exit: Exit::Success,
            text: render(&lines),
            json: Some(json!({
                "ticket": ticket,
                "commit": record.commit,
                "note": note,
                "passes": summary.map(|s| s.passes),
                "from_cache": summary.map(|s| s.gated.run.from_cache),
                "ran": summary.map(|s| s.gated.run.ran),
                "left_to_tip": summary.map(|s| s.gated.left),
                "checks_s": summary.map(|s| s.gated.run.wall_s),
                "build_s": summary.map(|s| s.gated.run.builds.iter().map(|b| b.wall_s).sum::<f64>()),
                "budget_billions": budget.map(crate::checks::GateBudget::billions),
                "rate": budget.map(|b| b.rate),
                "closed": closed == "ticket closed",
                "deploy": deploy,
                "express": summary.is_some_and(|s| s.express.is_some()),
                "formatted": summary.and_then(|s| s.formatted.as_ref()).and_then(|f| f.commit.clone()),
                "express_carried": summary.is_some_and(|s| s.express.as_ref().is_some_and(|r| r.carried)),
                "landing": summary.map(|s| s.timeline.json(ctx.clock.now())),
            })),
            handle: None,
        }
    }
}
