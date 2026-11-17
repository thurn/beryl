//! `test`: every interface test whose name starts with a prefix, at the check times or on every
//! tick (docs/beryl-3d.md, "Test speed"). The tests themselves are in [`crate::runner`].

use std::collections::BTreeSet;
use std::fmt::Write;
use std::io::Write as _;
use std::time::{Duration, Instant};

use beryl_scene::agent::{Context, Judged, Rule};
use beryl_scene::driver::Mode;
use serde_json::{Value, json};

use super::options::{self, Options, Usage};
use super::{Printed, milliseconds};
use crate::faults::{Changes, Comparison, Listed};
use crate::pages::{self, Windows};
use crate::runner::{self, Frames, Limits, Outcome, Test};

/// How many faults `test` prints before it gives a count of the rest.
const MOST_FAULTS: usize = 20;

/// How many of the slowest tests the report names.
const SLOWEST: usize = 3;

/// The switch that checks every tick instead of the check times.
pub(super) const EVERY_TICK: &str = "--every-tick";

/// The switch that lowers the entries of `data/beryl3d/test-faults.json` to what was found.
pub(super) const UPDATE: &str = "--update";

/// The switch that, beside [`UPDATE`], also raises and adds entries.
pub(super) const RAISE: &str = "--raise";

/// The option that stops the run from starting a test after this many seconds. The fast check
/// passes the time left before its hard limit, which kills a step still running and reports
/// nothing of it: after a long shared build the step was cut off in 141 of 330 fast checks on
/// 2026-11-16. With this option the tests already started finish, the rest are named as
/// deferred to the Beryl 3D sweep, and the run is judged as a prefix's run is, on the tests it
/// played.
pub(super) const STOP_AFTER: &str = "--stop-after";

/// The switch that plays the phone windows of the matrix instead of the desktop ones.
pub(super) const PHONE_ONLY: &str = "--phone-only";

/// The switch that prints the names of the tests the rest of the command line would play, one
/// a line, and plays none (`just b3d match boxes --all` reads it).
pub(super) const LIST: &str = "--list";

/// How many rules the closing summary names as having the most listed faults.
const MOST_RULES: usize = 3;

/// One fault or error, as printed, and how many more tests found it.
struct Found {
    /// The printed fault, with the test that found it first.
    text: String,
    /// What the JSON output gives for it.
    row: Value,
    /// What makes two tests' findings the same: the recording, then the rule and the elements,
    /// or the error.
    key: (String, String),
    more: usize,
}

pub(super) fn test(options: &Options) -> Result<Printed, Usage> {
    let prefix = match options.rest.as_slice() {
        [] => "",
        [prefix] => prefix.as_str(),
        _ => {
            return Err(Usage(format!(
                "test takes at most one prefix of a test's name, not {}",
                options.rest.join(" ")
            )));
        }
    };
    let (mode, windows) = switches(options)?;
    let phone_only = windows == Windows::Phone;
    let stop = stop(options)?;
    let tests = runner::tests(prefix, windows).map_err(Usage)?;
    if tests.is_empty() {
        return Err(nothing_matches(prefix)?);
    }
    if options.switch(LIST) {
        let names: String = tests.iter().map(|test| test.name() + "\n").collect();
        return Ok(Printed::text(names));
    }
    let limits = match mode {
        Mode::AtCheckTimes => Some(Limits::read().map_err(Usage)?),
        Mode::EveryTick => None,
    };
    // Both modes are held to the one list: the differential test requires that the check times
    // find every fault the every-tick run finds.
    let mut listed = Some(Listed::read().map_err(Usage)?);
    let started = Instant::now();
    let outcomes = runner::run_until(&tests, mode, stop);
    let seconds = started.elapsed().as_secs_f64();
    let deferred = tests.len() - outcomes.len();
    // The median is only held over the whole run: a prefix, or the tests that started before
    // `--stop-after`, are not typical. A phone run is never every test, so no desktop entry of
    // the list counts as not a test.
    let every_test = prefix.is_empty() && deferred == 0 && !phone_only;
    let over = limits.map_or_else(Vec::new, |limits| limits.over(&outcomes, every_test));
    let mut changes = None;
    if let Some(listed) = listed.as_mut().filter(|_| options.switch(UPDATE)) {
        let done = listed.update(&outcomes, every_test, options.switch(RAISE));
        if done.any() {
            listed.write().map_err(Usage)?;
        }
        changes = Some(done);
    }
    let update = format!(
        "just b3d test{} {UPDATE}",
        if prefix.is_empty() {
            String::new()
        } else {
            format!(" {prefix}")
        }
    );
    let comparison = listed
        .as_ref()
        .map(|listed| listed.compare(&outcomes, every_test, &update));
    // What is printed is each fault the list does not allow, not the thousands it does.
    let shown: Vec<Outcome> = match &listed {
        Some(listed) => outcomes
            .iter()
            .map(|o| Outcome {
                faults: listed.excess(o),
                ..o.clone()
            })
            .collect(),
        None => outcomes.clone(),
    };
    let mut slowest: Vec<&Outcome> = outcomes.iter().collect();
    slowest.sort_by(|a, b| b.seconds.total_cmp(&a.seconds));
    slowest.truncate(SLOWEST);
    let failed = shown.iter().filter(|o| !o.passed()).count()
        + comparison.as_ref().map_or(0, |c| {
            let mut tests: Vec<&str> = c.over.iter().map(|(t, _)| t.as_str()).collect();
            tests.dedup();
            tests.len()
        });
    let failing = failing(&shown, comparison.as_ref());
    log(&outcomes, &failing);
    let report = Report {
        command: options.command_line(),
        found: grouped(&shown),
        over,
        comparison: comparison.unwrap_or_default(),
        summary: listed.as_ref().map(|l| l.summary(MOST_RULES)),
        changes,
        slowest,
        frames: Frames::of(&outcomes),
        failed,
        outcomes: &outcomes,
        failing,
        deferred,
        seconds,
    };
    let fault = report.failed > 0 || !report.over.is_empty() || !report.comparison.high.is_empty();
    let text = if options.json {
        report.json()
    } else {
        report.text()
    };
    Ok(Printed { text, fault })
}

/// When [`STOP_AFTER`] says the run stops starting tests, if it was given.
fn stop(options: &Options) -> Result<Option<Instant>, Usage> {
    let Some(text) = options.value(STOP_AFTER) else {
        return Ok(None);
    };
    if options.switch(UPDATE) {
        return Err(Usage(format!(
            "{UPDATE} needs every test's faults, so it does not go with {STOP_AFTER}"
        )));
    }
    match text.parse::<f64>() {
        Ok(seconds) if seconds.is_finite() => Ok(Some(
            Instant::now() + Duration::from_secs_f64(seconds.max(0.0)),
        )),
        _ => Err(Usage(format!(
            "{STOP_AFTER} takes a number of seconds, not {text}"
        ))),
    }
}

/// The mode and the windows `test`'s switches choose.
///
/// # Errors
///
/// When two switches do not go together.
fn switches(options: &Options) -> Result<(Mode, Windows), Usage> {
    if options.switch(RAISE) && !options.switch(UPDATE) {
        return Err(Usage(format!("{RAISE} goes beside {UPDATE}")));
    }
    if options.switch(UPDATE) && options.switch(EVERY_TICK) {
        return Err(Usage(format!(
            "{UPDATE} lists the faults found at the check times, so it does not go with \
             {EVERY_TICK}"
        )));
    }
    // The phone layout is still being built, and its faults are fixed rather than listed, so a
    // phone run never writes the list: it could only add phone entries or drop desktop ones.
    if options.switch(PHONE_ONLY) && options.switch(UPDATE) {
        return Err(Usage(format!(
            "{UPDATE} lists the desktop tests' faults, so it does not go with {PHONE_ONLY}"
        )));
    }
    let windows = if options.switch(PHONE_ONLY) {
        Windows::Phone
    } else {
        Windows::Desktop
    };
    let mode = if options.switch(EVERY_TICK) {
        Mode::EveryTick
    } else {
        Mode::AtCheckTimes
    };
    Ok((mode, windows))
}

/// What a run of tests found, to be printed.
struct Report<'a> {
    /// The command that runs the same tests again.
    command: String,
    outcomes: &'a [Outcome],
    found: Vec<Found>,
    /// What is over the frame limits, one sentence each.
    over: Vec<String>,
    /// How the faults compare with `data/beryl3d/test-faults.json`; empty when it is not held.
    comparison: Comparison,
    /// How many tests have a listed fault and the rules with the most, when the list is held.
    summary: Option<(usize, Vec<(String, usize)>)>,
    /// What `--update` changed in the list.
    changes: Option<Changes>,
    slowest: Vec<&'a Outcome>,
    frames: Option<Frames>,
    failed: usize,
    /// The names of the tests that fail ([`failing`]), for the JSON output's `results`.
    failing: BTreeSet<String>,
    /// How many tests had not started when `--stop-after` passed.
    deferred: usize,
    /// The run's time on the clock.
    seconds: f64,
}

impl Report<'_> {
    fn frames(&self) -> usize {
        self.outcomes.iter().map(|o| o.frames).sum()
    }

    /// What each rule judged over every test.
    fn judged(&self) -> Judged {
        let mut judged = Judged::default();
        for outcome in self.outcomes {
            judged += &outcome.judged;
        }
        judged
    }

    fn median(&self) -> usize {
        self.frames.as_ref().map_or(0, |f| f.median)
    }

    /// The tests' time on their threads' clocks, in milliseconds a test.
    fn test_ms(&self) -> f64 {
        let seconds: f64 = self.outcomes.iter().map(|o| o.seconds).sum();
        seconds * 1000.0 / self.outcomes.len().max(1) as f64
    }

    fn json(&self) -> String {
        let value = json!({
            "command": self.command,
            "tests": self.outcomes.len(),
            "deferred": self.deferred,
            "failed": self.failed,
            "frames": self.frames(),
            "median_frames": self.median(),
            "most_frames": self.frames.as_ref().map(|f| f.most),
            "costliest": self.frames.as_ref().map(|f| f.costliest.clone()),
            "faults": self.found.iter().map(|f| {
                let mut row = f.row.clone();
                row["more_tests"] = json!(f.more);
                row
            }).collect::<Vec<_>>(),
            "over_limits": self.over,
            "over_listed": self.comparison.over.iter().map(|(_, why)| why).collect::<Vec<_>>(),
            "listed_too_high": self.comparison.high.iter().map(|(_, why)| why).collect::<Vec<_>>(),
            "listed_tests": self.summary.as_ref().map(|(tests, _)| *tests),
            "listed_rules": self.summary.as_ref().map(|(_, rules)| {
                rules.iter().map(|(rule, n)| json!({ "rule": rule, "faults": n })).collect::<Vec<_>>()
            }),
            "updated": self.changes.as_ref().map(|c| json!({
                "lowered": c.lowered,
                "removed": c.removed,
                "raised": c.raised,
                "added": c.added,
            })),
            "slowest": self.slowest.iter().map(|o| json!({
                "test": o.test.name(),
                "seconds": o.seconds,
                "frames": o.frames,
            })).collect::<Vec<_>>(),
            // Every test played and whether it passed, as the run log has them, so that
            // `just b3d parity` can tell which recordings pass from both seats in every window.
            "results": self.outcomes.iter().map(|o| {
                let name = o.test.name();
                let outcome = if self.failing.contains(&name) { "fail" } else { "pass" };
                json!({ "test": name, "outcome": outcome })
            }).collect::<Vec<_>>(),
            "seconds": self.seconds,
            "test_ms": self.test_ms(),
            "judged": Rule::ALL.iter().map(|&rule| {
                let count = self.judged().of(rule);
                json!({ "rule": rule.word(), "frames": count.frames, "elements": count.elements })
            }).collect::<Vec<_>>(),
        });
        format!("{value}\n")
    }

    fn text(&self) -> String {
        let mut out = format!("{}\n", self.command);
        for f in self.found.iter().take(MOST_FAULTS) {
            let mut lines = f.text.lines();
            let first = lines.next().unwrap_or_default();
            if f.more > 0 {
                let _ = writeln!(out, "{first}  (and {} more tests)", f.more);
            } else {
                let _ = writeln!(out, "{first}");
            }
            for line in lines {
                let _ = writeln!(out, "{line}");
            }
        }
        if self.found.len() > MOST_FAULTS {
            let _ = writeln!(out, "... {} more faults", self.found.len() - MOST_FAULTS);
        }
        for line in &self.over {
            let _ = writeln!(out, "OVER {line}");
        }
        for (_, line) in self.comparison.over.iter().take(MOST_FAULTS) {
            let _ = writeln!(out, "OVER {line}");
        }
        for (_, line) in self.comparison.high.iter().take(MOST_FAULTS) {
            let _ = writeln!(out, "TOO HIGH {line}");
        }
        if let Some(c) = &self.changes {
            if c.any() {
                let _ = writeln!(
                    out,
                    "updated data/beryl3d/test-faults.json: {} lowered, {} removed, {} raised, \
                     {} added",
                    c.lowered, c.removed, c.raised, c.added
                );
            } else {
                let _ = writeln!(out, "data/beryl3d/test-faults.json is unchanged");
            }
        }
        if self.outcomes.len() > SLOWEST {
            let named: Vec<String> = self
                .slowest
                .iter()
                .map(|o| format!("{} {:.1}s", o.test.name(), o.seconds))
                .collect();
            let _ = writeln!(out, "slowest: {}", named.join(", "));
        }
        let judged = self.judged();
        let counts: Vec<String> = Rule::ALL
            .iter()
            .map(|&rule| {
                let count = judged.of(rule);
                format!("{} {}/{}", rule.word(), count.frames, count.elements)
            })
            .collect();
        let _ = writeln!(
            out,
            "judged, in frames (scripts for the segment checks) / elements: {}",
            counts.join(", ")
        );
        let count =
            |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        let _ = writeln!(
            out,
            "{}, {} failed, {}; {} frames, median {} a test; {:.1}s, {:.1} test-ms a test",
            count(self.outcomes.len(), "test", "tests"),
            self.failed,
            count(self.found.len(), "different fault", "different faults"),
            self.frames(),
            self.median(),
            self.seconds,
            self.test_ms(),
        );
        if self.deferred > 0 {
            let _ = writeln!(
                out,
                "{} deferred to the Beryl 3D sweep: not started before {STOP_AFTER} passed",
                count(self.deferred, "test", "tests"),
            );
        }
        if let Some((tests, rules)) = &self.summary {
            let most: Vec<String> = rules
                .iter()
                .map(|(rule, n)| format!("{rule} ({n})"))
                .collect();
            let _ = writeln!(
                out,
                "{} a listed fault{}",
                count(*tests, "test has", "tests have"),
                if most.is_empty() {
                    String::new()
                } else {
                    format!("; most listed: {}", most.join(", "))
                },
            );
        }
        out
    }
}

/// Every fault and error in `outcomes`, each once with the number of other tests that found
/// it, in the order the tests are named.
fn grouped(outcomes: &[Outcome]) -> Vec<Found> {
    let mut found: Vec<Found> = Vec::new();
    let mut add = |f: Found| match found.iter_mut().find(|known| known.key == f.key) {
        Some(known) => known.more += 1,
        None => found.push(f),
    };
    for outcome in outcomes {
        let test = &outcome.test;
        if let Some(why) = &outcome.error {
            add(Found {
                text: format!("ERROR {}\n  {why}", test.name()),
                row: json!({ "test": test.name(), "error": why }),
                key: (test.recording.clone(), format!("error {why}")),
                more: 0,
            });
        }
        let context = context(test);
        for fault in &outcome.faults {
            let locators: Vec<&str> = fault.elements.iter().map(|e| e.locator.as_str()).collect();
            add(Found {
                text: fault.print(&context),
                row: json!({
                    "test": test.name(),
                    "rule": fault.rule.word(),
                    "t_ms": milliseconds(fault.at),
                    "message": fault.message,
                    "elements": locators,
                }),
                key: (
                    test.recording.clone(),
                    format!("{} {}", fault.rule.word(), locators.join(" and ")),
                ),
                more: 0,
            });
        }
    }
    found
}

/// What a fault of `test`'s is printed with.
fn context(test: &Test) -> Context {
    Context {
        recording: test.recording.clone(),
        seat: test.seat,
        layout: test.layout(),
        size: test.size,
        safe_area: pages::safe_area(test.layout()),
        at: 0,
    }
}

/// The names of the tests that fail: those that errored or have a fault the list does not
/// allow (`shown`), a count over its entry, or an entry that is now too high.
fn failing(shown: &[Outcome], comparison: Option<&Comparison>) -> BTreeSet<String> {
    let mut failing: BTreeSet<String> = shown
        .iter()
        .filter(|o| !o.passed())
        .map(|o| o.test.name())
        .collect();
    if let Some(comparison) = comparison {
        failing.extend(comparison.over.iter().map(|(name, _)| name.clone()));
        failing.extend(comparison.high.iter().map(|(name, _)| name.clone()));
    }
    failing
}

/// Gives each test's result to the run log, through the file `$BERYL_CHECK_RESULTS` names,
/// when a run of the checks asked for results. Logging never fails the tests.
fn log(outcomes: &[Outcome], failing: &BTreeSet<String>) {
    let Some(path) = std::env::var_os("BERYL_CHECK_RESULTS") else {
        return;
    };
    let logged = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| file.write_all(runner::results(outcomes, failing).as_bytes()));
    if let Err(e) = logged {
        eprintln!("run log: not written: {e}");
    }
}

/// The error for a prefix no test's name starts with, naming the nearest recordings.
fn nothing_matches(prefix: &str) -> Result<Usage, Usage> {
    let names = crate::recordings::names().map_err(Usage)?;
    let mut unique: Vec<&str> = names.iter().map(|(n, _)| n.as_str()).collect();
    unique.dedup();
    let wanted = runner::full_name(prefix);
    let recording = wanted.trim_start_matches(runner::PREFIX);
    Ok(Usage(format!(
        "no test's name starts with '{wanted}'. Tests are named \
         b3d:<recording>/<seat>/<width>x<height>; the nearest recordings are:\n{}",
        options::nearest(recording, &unique)
            .iter()
            .map(|n| format!("  {n}"))
            .collect::<Vec<_>>()
            .join("\n")
    )))
}
