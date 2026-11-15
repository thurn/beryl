//! `check`: every interface check on every tick of a recording, for one seat and window.

use std::fmt::Write;
use std::time::Instant;

use beryl_scene::agent::{Context, Fault};
use beryl_scene::driver::Mode;
use beryl_scene::input::Insets;
use serde_json::{Value, json};

use super::options::{Options, Usage};
use super::{Printed, milliseconds, open};
use crate::pages::LEFT_OUT;

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
    for fault in checked.faults {
        if LEFT_OUT.contains(&fault.rule) {
            continue;
        }
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
        size: options.size,
        safe_area: Insets::default(),
        at: 0,
    };
    let seconds = started.elapsed().as_secs_f64();
    let left_out: Vec<&str> = LEFT_OUT.iter().map(|rule| rule.word()).collect();
    let closing = format!(
        "1 test, {} frames, {} faults in {seconds:.1}s ({} left out)",
        checked.frames,
        found.len(),
        left_out.join(", ")
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
