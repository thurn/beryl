//! `check`: every interface check on every tick of a recording, for one seat and window.

use std::fmt::Write;
use std::time::Instant;

use app_client::driver::Mode;
use kit_probe::agent::{Context, Fault};
use serde_json::{Value, json};

use super::options::{Options, Usage};
use super::{Printed, milliseconds, open};
use crate::stress;

/// How many faults `check` prints before it gives a count of the rest.
const MOST_FAULTS: usize = 20;

/// One fault, and how many more times it was found about the same elements.
struct Found {
    fault: Fault,
    more: usize,
}

pub(super) fn check(options: &Options) -> Result<Printed, Usage> {
    if !options.actions.is_empty() {
        return Err(Usage(
            "check plays the recording with no action taken, so it takes no --do; use tree or \
             diff to see what an action does"
                .to_owned(),
        ));
    }
    let started = Instant::now();
    let mut page = open(options)?;
    let checked = page.check_all(Mode::EveryTick);
    let mut found: Vec<Found> = Vec::new();
    let audited = checked
        .faults
        .into_iter()
        .filter(|fault| stress::audited(options.name(), fault.rule));
    for fault in audited {
        let same = found.iter_mut().find(|known| {
            known.fault.rule == fault.rule
                && known
                    .fault
                    .elements
                    .iter()
                    .map(|e| &e.locator)
                    .eq(fault.elements.iter().map(|e| &e.locator))
        });
        match same {
            Some(known) => known.more += 1,
            None => found.push(Found { fault, more: 0 }),
        }
    }
    let context = Context {
        recording: options.name().to_owned(),
        seat: options.seat,
        layout: options.layout,
        checks: crate::pages::layout_checks(options.layout, options.size),
        size: options.size,
        safe_area: crate::pages::safe_area(options.layout),
        at: 0,
    };
    let seconds = started.elapsed().as_secs_f64();
    let closing = format!(
        "1 test, {} frames, lines-hold on {} pairs, {} faults in {seconds:.1}s",
        checked.frames,
        checked.line_pairs,
        found.len()
    );
    let fault = !found.is_empty();
    if options.json {
        let rows: Vec<Value> = found
            .iter()
            .map(|f| {
                let printed = f.fault.print(&context);
                json!({
                    "rule": f.fault.rule.word(),
                    "t_ms": milliseconds(f.fault.at),
                    "more": f.more,
                    "message": f.fault.message,
                    "elements": f.fault.elements.iter().map(|e| e.locator.clone()).collect::<Vec<_>>(),
                    "show": printed.lines().last().and_then(|l| l.trim().strip_prefix("show: ")),
                })
            })
            .collect();
        let value = json!({
            "command": options.command_line(),
            "tests": 1,
            "frames": checked.frames,
            "line_pairs": checked.line_pairs,
            "faults": rows,
            "seconds": seconds,
        });
        return Ok(Printed {
            text: format!("{value}\n"),
            fault,
        });
    }
    let mut out = format!("{}\n", options.command_line());
    for f in found.iter().take(MOST_FAULTS) {
        let printed = f.fault.print(&context);
        let mut lines = printed.lines();
        let first = lines.next().unwrap_or_default();
        if f.more > 0 {
            let _ = writeln!(out, "{first}  (and {} more times)", f.more);
        } else {
            let _ = writeln!(out, "{first}");
        }
        for line in lines {
            let _ = writeln!(out, "{line}");
        }
    }
    if found.len() > MOST_FAULTS {
        let _ = writeln!(out, "... {} more faults", found.len() - MOST_FAULTS);
    }
    let _ = writeln!(out, "{closing}");
    Ok(Printed { text: out, fault })
}
