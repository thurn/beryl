//! The part of `ignite why` that reads only the hub's record of a check: its family, runs,
//! failures, cost and state. Outside a build tree, `ignite why` prints only this part, since
//! listing the checks of a tree and hashing their inputs may build products, and reading a
//! record must change nothing.

use ignite_core::manifest::Kind;
use ignite_core::protocol::client::{Exit, Reply};
use ignite_core::protocol::hub::{CheckState, CheckStats};

use super::Checks;
use super::verbs::{matches, render};

/// The lines that name a check, its family and its record.
pub(super) fn head_lines(
    checks: &Checks,
    check: &str,
    family_name: &str,
    stats: Option<&CheckStats>,
) -> Vec<(String, String)> {
    let mut lines = vec![("check".to_owned(), check.to_owned())];
    if let Some(declared) = checks.settings.manifest.families.get(family_name) {
        let family = &declared.family;
        let kind = match family.kind {
            Kind::Regression => "regression",
            Kind::Exploration => "exploration",
        };
        let platforms = if family.platforms.is_any() {
            "any platform".to_owned()
        } else {
            format!("on {}", checks.settings.os.as_str())
        };
        let always = if family.always { ", always set" } else { "" };
        lines.push((
            "family".to_owned(),
            format!("{family_name} ({kind}, {platforms}{always})"),
        ));
    }
    let record = match stats {
        Some(s) => {
            let runs: u32 = s.areas.iter().map(|a| a.runs).sum();
            let failures: u32 = s.areas.iter().map(|a| a.failures).sum();
            let cost = s.median_cost.map_or_else(String::new, |c| {
                let unit = if s.unit == "cpu_s" {
                    "CPU s"
                } else {
                    "billion"
                };
                format!("; median cost {c} {unit}")
            });
            format!("{failures} failures in {runs} runs{cost}")
        }
        None => "never run".to_owned(),
    };
    lines.push(("record".to_owned(), record));
    if let Some(s) = stats {
        let state_text = match s.state {
            CheckState::On => "on",
            CheckState::Off => "off: switched off as flaky",
            CheckState::Uncached => "uncached: an audit disagreed",
        };
        lines.push(("state".to_owned(), state_text.to_owned()));
    }
    lines
}

/// `ignite why <check>` from outside any build tree: the record of the one check the hub knows
/// that matches `wanted`, found among the checks of the family its name starts with, or of
/// every family. `--inputs`, the cache and the gate's choice need a tree.
pub(super) fn outside_a_tree(checks: &Checks, wanted: &str, inputs: bool) -> Reply {
    if inputs {
        return Reply::text(
            Exit::Usage,
            "ignite why --inputs reads a tree's inputs: run it in a build tree",
        );
    }
    let families: Vec<&str> = match wanted.split_once(':') {
        Some((family, _)) if checks.families().any(|(n, _)| n == family) => vec![family],
        _ => checks.families().map(|(name, _)| name).collect(),
    };
    let mut found: Vec<(String, CheckStats)> = Vec::new();
    for family in families {
        let Ok(reply) = checks.history.stats(family) else {
            continue;
        };
        for stats in reply.checks {
            if matches(wanted, stats.check.as_str()) {
                found.push((family.to_owned(), stats));
            }
        }
    }
    let exact = found.iter().position(|(_, s)| s.check.as_str() == wanted);
    let (family, stats) = match (exact, found.len()) {
        (Some(index), _) => found.swap_remove(index),
        (None, 1) => found.remove(0),
        (None, 0) => {
            return Reply::text(Exit::Usage, format!("the hub has no record of {wanted}"));
        }
        (None, count) => {
            let names: Vec<&str> = found
                .iter()
                .take(5)
                .map(|(_, s)| s.check.as_str())
                .collect();
            return Reply::text(
                Exit::Usage,
                format!("{count} checks match {wanted}: {}", names.join(", ")),
            );
        }
    };
    let mut lines = head_lines(checks, stats.check.as_str(), &family, Some(&stats));
    lines.push((
        "runs".to_owned(),
        "depends on the change: run ignite why in a build tree to see the gate's choice".to_owned(),
    ));
    Reply::text(Exit::Success, render(&lines))
}
