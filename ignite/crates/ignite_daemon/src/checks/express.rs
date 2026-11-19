//! `ignite test` and `ignite check` on the express cores: a tree that changes only Ignite runs
//! the express commands in place of the checks of `[express] families` (see "Express landings"
//! in the design document and [`crate::land::express`]).
//!
//! On 2026-11-18 `ignite test` of Ignite's own checks took 5 to 8.5 minutes while other builds
//! held the scheduler's cores: the test build got 1 to 4 cores and queued behind builds costing
//! 1,000 CPU-seconds or more, against about a minute on 12 cores. So when every changed file of
//! the tree matches `[express] paths`, the verbs run the express commands in the agent's own
//! tree, on the machine's `express_cores`, which the scheduler never hands out. They take the
//! express lane's lock, so a test waits for an express landing that holds it rather than going
//! back to the scheduler. `ignite test` does so only when every name it is given is in one of
//! those families (`ignite:...`, `ignite-lint:...`), and runs every command, whatever names it
//! was given. `ignite check` does so for any such tree, because the landing would take the
//! express lane too.

use std::sync::Arc;

use serde_json::json;

use ignite_core::protocol::client::{Exit, Reply};

use super::OpenTree;
use super::verbs::{changed_paths, render};
use crate::checks::Checks;
use crate::land::express::{ExpressLane, ExpressRun};
use crate::server::Context;

/// What the verb was asked to run.
#[derive(Clone, Copy, Debug)]
pub(super) enum Asked<'a> {
    /// `ignite test` of these names.
    Named(&'a [&'a str]),
    /// `ignite check`: what the landing gate would run.
    Gate,
}

/// The reply of an express run in `tree`, when the machine has an express lane, the repository
/// names the families its commands stand in for, `asked` is only checks of those families and
/// every changed file of the tree matches `[express] paths`. `None` leaves the verb to run the
/// checks as usual.
pub(super) fn express_reply(
    lane: Option<&Arc<dyn ExpressLane>>,
    checks: &Checks,
    ctx: &Context,
    tree: &OpenTree,
    asked: Asked<'_>,
) -> Option<Reply> {
    let lane = lane?;
    let express = ctx.config.express.as_ref()?;
    if express.families.is_empty() || express.commands.is_empty() {
        return None;
    }
    let ours = |word: &&str| {
        word.split_once(':')
            .is_some_and(|(family, _)| express.families.iter().any(|f| f == family))
    };
    let what = match asked {
        Asked::Named(words) if !words.is_empty() && words.iter().all(ours) => {
            format!("the checks of {}", express.families.join(" and "))
        }
        Asked::Named(_) => return None,
        Asked::Gate => "the gate's checks".to_owned(),
    };
    let changed = changed_paths(checks, ctx, tree).ok()?;
    if !express.covers(&changed) {
        return None;
    }
    Some(reply(&lane.run_in(&tree.path), &what))
}

/// The reply to an express run: the `express` line when every command passed, else exit code
/// 2 with the failed command and the end of its output.
fn reply(run: &ExpressRun, what: &str) -> Reply {
    let ran: Vec<_> = run
        .ran
        .iter()
        .map(|(command, seconds)| json!({ "command": command, "seconds": seconds }))
        .collect();
    let Some((command, output)) = run.failure.clone() else {
        return Reply {
            exit: Exit::Success,
            text: render(&[run.line_in_place_of(what)]),
            json: Some(json!({
                "express": { "ran": ran, "cores": run.cores, "reused_after_s": run.reused_after_s }
            })),
            handle: None,
        };
    };
    let mut lines = vec![(
        "failed".to_owned(),
        format!(
            "`{command}` failed on {} cores, in place of {what}",
            run.cores
        ),
    )];
    lines.extend(output.lines().map(|text| (String::new(), text.to_owned())));
    Reply {
        exit: Exit::CheckFailed,
        text: render(&lines),
        json: Some(json!({
            "express": { "ran": ran, "cores": run.cores, "command": command, "output": output }
        })),
        handle: None,
    }
}
