//! The daemon's background loops, in `ignite status` and `ignite doctor`.
//!
//! The loops report each round to a [`LoopHealth`]. `status` shows one line per loop, and
//! `doctor` reports a problem for a loop that has not finished a round for far longer than it
//! should, or that has failed many rounds in a row.

use super::{Finding, StatusSection};
use crate::loop_health::LoopHealth;

/// One line per loop in `ignite status`.
pub struct LoopsSection(pub LoopHealth);

impl StatusSection for LoopsSection {
    fn title(&self) -> &'static str {
        "loops"
    }

    fn lines(&self) -> Vec<String> {
        self.0.lines()
    }
}

/// The `loops` findings of `doctor`: one for each loop that is stuck or failing, or one line
/// saying all are well. Nothing for a daemon that has started no loops.
///
/// A loop whose round is slow but names what it is doing or waiting for, such as a build or
/// the cores it waits for, is a warning that names it: restarting the daemon would only start
/// that again. Only a loop that names nothing gets the advice to restart.
pub fn findings(health: &LoopHealth) -> Vec<Finding> {
    let reports = health.reports();
    if reports.is_empty() {
        return Vec::new();
    }
    let now = health.now();
    let findings: Vec<Finding> = reports
        .iter()
        .filter_map(|report| {
            let problem = report.problem(now)?;
            Some(if report.accounted_for() {
                Finding::warning(
                    "loops",
                    format!("{problem}."),
                    "look at what the round names; `ignite status` shows its step and \
                     `ignite status --admission` what holds the cores. Restarting the daemon \
                     would only start the round again",
                )
            } else {
                Finding::problem(
                    "loops",
                    format!("{problem}."),
                    "restart the daemon, and if it happens again read its log and the crash \
                     records in the `crash` folder of the Ignite home",
                )
            })
        })
        .collect();
    if findings.is_empty() {
        return vec![Finding::ok(
            "loops",
            format!("{} background loops are making progress.", reports.len()),
        )];
    }
    findings
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use ignite_core::fakes::FakeClock;
    use ignite_core::time::Timestamp;

    use super::*;

    #[test]
    fn a_stuck_loop_is_a_loops_problem_and_a_working_one_is_not() {
        let clock = Arc::new(FakeClock::new(Timestamp::EPOCH));
        let health = LoopHealth::new(clock.clone());
        assert!(findings(&health).is_empty());
        let sender = health.register("ignite-sender", Duration::from_mins(10));
        health.register("ignite-tip", Duration::from_hours(3));
        clock.advance(300.0);
        sender.round(|| Ok::<_, String>(()));
        let well = findings(&health);
        assert_eq!(well.len(), 1);
        assert!(well[0].ok && well[0].label == "loops", "{well:?}");

        clock.advance(700.0);
        let stuck = findings(&health);
        assert_eq!(stuck.len(), 1, "{stuck:?}");
        assert!(
            !stuck[0].ok && stuck[0].text.contains("ignite-sender"),
            "{stuck:?}"
        );
        assert!(stuck[0].line().starts_with("PROBLEM  loops"));
    }

    #[test]
    fn a_slow_round_that_names_what_it_waits_for_is_a_warning_without_a_restart() {
        let clock = Arc::new(FakeClock::new(Timestamp::EPOCH));
        let health = LoopHealth::new(clock.clone());
        let idle = health.register("ignite-idle", Duration::from_mins(10));
        let (entered, release) = (
            Arc::new(std::sync::Barrier::new(2)),
            Arc::new(std::sync::Barrier::new(2)),
        );
        let round = {
            let (entered, release) = (entered.clone(), release.clone());
            std::thread::spawn(move || {
                idle.round(|| {
                    crate::steps::step("prebuilding tree-03 at 7322d14");
                    let _cores = crate::steps::waiting("8 cores for idle work");
                    entered.wait();
                    release.wait();
                    Ok::<_, String>(())
                })
            })
        };
        entered.wait();
        clock.advance(21.0 * 60.0);
        let found = findings(&health);
        release.wait();
        round.join().unwrap();
        assert_eq!(found.len(), 1, "{found:?}");
        let line = found[0].line();
        assert!(line.starts_with("WARNING  loops"), "{line}");
        assert!(
            line.contains(
                "a round is still running: prebuilding tree-03 at 7322d14 (21 min), waiting for \
                 8 cores for idle work (21 min)"
            ),
            "{line}"
        );
        assert!(!line.contains("restart the daemon,"), "{line}");
    }

    #[test]
    fn the_status_section_has_a_line_per_loop() {
        let clock = Arc::new(FakeClock::new(Timestamp::EPOCH));
        let health = LoopHealth::new(clock);
        health.register("ignite-idle", Duration::from_mins(10));
        let lines = LoopsSection(health).lines();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("ignite-idle"), "{lines:?}");
    }
}
