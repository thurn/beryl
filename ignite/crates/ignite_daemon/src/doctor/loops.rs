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

/// The `loops` findings of `doctor`: one problem for each loop that is stuck or failing, or one
/// line saying all are well. Nothing for a daemon that has started no loops.
pub fn findings(health: &LoopHealth) -> Vec<Finding> {
    let loops = health.reports().len();
    if loops == 0 {
        return Vec::new();
    }
    let problems = health.problems();
    if problems.is_empty() {
        return vec![Finding::ok(
            "loops",
            format!("{loops} background loops are making progress."),
        )];
    }
    problems
        .into_iter()
        .map(|problem| {
            Finding::problem(
                "loops",
                format!("{problem}."),
                "restart the daemon, and if it happens again read its log and the crash records \
                 in the `crash` folder of the Ignite home",
            )
        })
        .collect()
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
    fn the_status_section_has_a_line_per_loop() {
        let clock = Arc::new(FakeClock::new(Timestamp::EPOCH));
        let health = LoopHealth::new(clock);
        health.register("ignite-idle", Duration::from_mins(10));
        let lines = LoopsSection(health).lines();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("ignite-idle"), "{lines:?}");
    }
}
