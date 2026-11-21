//! Admission: whether a ticket's expected use of cores fits beside the tickets in progress.
//!
//! A ticket's expected cores is a figure from history: over the tickets landed in the last 30
//! days that named files in the same areas, the median of core-minutes used per minute worked.
//! An area is a workspace package or a second-level directory. A ticket that names several areas
//! takes the largest figure, one that names none takes the machine's median, and a planner's
//! `--cores` replaces the figure. A machine admits a ticket when the tickets in progress there,
//! each counted at the larger of its expectation and its use so far, plus the new ticket's
//! figure fit within the machine's `admit_cores`. A regression ticket skips this test; dispatch
//! applies that exception.

use crate::time::Timestamp;

/// How far back history counts.
pub const HISTORY_DAYS: f64 = 30.0;

/// The figure when there is no history at all: one core.
pub const DEFAULT_CORES: f64 = 1.0;

/// Tickets below this estimate skip Rust prebuilding and take an older build tree.
pub const COMPILES_FROM_CORES: f64 = 0.5;

/// Slack for sums of floating-point figures, so that a ticket that fits exactly is admitted.
const EPSILON: f64 = 1e-9;

/// One landed ticket's use, as history for later expectations.
#[derive(Clone, Debug, PartialEq)]
pub struct Worked {
    pub landed_at: Timestamp,
    /// The areas of the files it named.
    pub areas: Vec<String>,
    pub core_minutes: f64,
    /// Minutes worked on it.
    pub minutes: f64,
}

/// The area of a repository path: the longest of `packages` (workspace package directories)
/// that holds it, or else its first two directories (a top-level file is its own area, as in
/// [`crate::inputs::area`]).
pub fn area_of(path: &str, packages: &[String]) -> String {
    let inside = |root: &&String| {
        let root = root.trim_end_matches('/');
        path.strip_prefix(root)
            .is_some_and(|rest| rest.starts_with('/'))
    };
    if let Some(root) = packages.iter().filter(inside).max_by_key(|root| root.len()) {
        return root.trim_end_matches('/').to_owned();
    }
    crate::inputs::directory_area(path)
}

/// The median, averaging the middle two of an even count; `None` for no values.
pub fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    Some(if sorted.len().is_multiple_of(2) {
        f64::midpoint(sorted[middle - 1], sorted[middle])
    } else {
        sorted[middle]
    })
}

/// A ticket's expected cores: the planner's figure when set; else the largest median, over the
/// ticket's `areas`, of core-minutes per minute worked by the tickets in `history` landed in the
/// last 30 days that named that area; else, when it names no area with history, the median of
/// all that history; else [`DEFAULT_CORES`]. `history` is the machine's.
pub fn expected_cores(
    planner: Option<f64>,
    areas: &[String],
    history: &[Worked],
    now: Timestamp,
) -> f64 {
    if let Some(cores) = planner {
        return cores;
    }
    let recent: Vec<&Worked> = history
        .iter()
        .filter(|w| w.minutes > 0.0 && now.seconds_since(w.landed_at) <= HISTORY_DAYS * 86_400.0)
        .collect();
    let rate = |w: &&Worked| w.core_minutes / w.minutes;
    let by_area = areas
        .iter()
        .filter_map(|area| {
            let rates: Vec<f64> = recent
                .iter()
                .filter(|w| w.areas.contains(area))
                .map(rate)
                .collect();
            median(&rates)
        })
        .reduce(f64::max);
    by_area
        .or_else(|| median(&recent.iter().map(rate).collect::<Vec<_>>()))
        .unwrap_or(DEFAULT_CORES)
}

/// How a ticket in progress counts toward the machine's total: the larger of its expectation
/// and the cores it has used so far.
pub fn counted(expected: f64, used_so_far: f64) -> f64 {
    expected.max(used_so_far)
}

/// The cores a machine can still admit beside `in_progress`, each already [`counted`].
pub fn free_cores(admit_cores: f64, in_progress: &[f64]) -> f64 {
    admit_cores - in_progress.iter().sum::<f64>()
}

/// Whether a ticket expecting `expected` cores fits under `admit_cores` beside `in_progress`,
/// each already [`counted`].
pub fn admits(admit_cores: f64, in_progress: &[f64], expected: f64) -> bool {
    fits(free_cores(admit_cores, in_progress), expected)
}

/// Whether `expected` fits in `free` cores.
pub fn fits(free: f64, expected: f64) -> bool {
    expected <= free + EPSILON
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: f64 = 86_400.0;

    fn worked(days_ago: f64, areas: &[&str], cores: f64, now: Timestamp) -> Worked {
        Worked {
            landed_at: now.plus_seconds(-days_ago * DAY),
            areas: areas.iter().map(|a| (*a).to_owned()).collect(),
            core_minutes: cores * 60.0,
            minutes: 60.0,
        }
    }

    fn areas(names: &[&str]) -> Vec<String> {
        names.iter().map(|a| (*a).to_owned()).collect()
    }

    #[test]
    fn a_tools_ticket_is_admitted_where_a_third_kernel_ticket_waits() {
        // jarvis: admit_cores 13, two kernel tickets and seven light ones in progress.
        let in_progress = [4.8, 4.6, 0.4, 0.3, 0.3, 0.3, 0.3, 0.3, 0.3];
        assert!((in_progress.iter().sum::<f64>() - 11.6).abs() < 1e-9);
        assert!(!admits(13.0, &in_progress, 2.1));
        assert!(admits(13.0, &in_progress, 0.1));
    }

    #[test]
    fn a_ticket_that_fits_exactly_is_admitted() {
        assert!(admits(13.0, &[6.5, 4.5], 2.0));
        assert!(!admits(13.0, &[6.5, 4.5], 2.01));
    }

    #[test]
    fn a_ticket_in_progress_counts_at_the_larger_of_expectation_and_use() {
        assert!((counted(2.0, 3.5) - 3.5).abs() < f64::EPSILON);
        assert!((counted(2.0, 0.5) - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn expected_cores_take_the_largest_area_median_over_30_days() {
        let now = Timestamp::from_millis(100 * 86_400_000);
        let history = [
            worked(1.0, &["rust/kernel"], 2.0, now),
            worked(2.0, &["rust/kernel"], 3.0, now),
            worked(3.0, &["rust/kernel"], 4.0, now),
            worked(1.0, &["tools/src"], 0.1, now),
            // Older than 30 days: not counted.
            worked(31.0, &["tools/src"], 9.0, now),
        ];
        let kernel = expected_cores(None, &areas(&["rust/kernel"]), &history, now);
        assert!((kernel - 3.0).abs() < 1e-12);
        let tools = expected_cores(None, &areas(&["tools/src"]), &history, now);
        assert!((tools - 0.1).abs() < 1e-12);
        let both = expected_cores(None, &areas(&["tools/src", "rust/kernel"]), &history, now);
        assert!((both - 3.0).abs() < 1e-12);
    }

    #[test]
    fn expected_cores_fall_back_to_the_machine_median_then_one_core() {
        let now = Timestamp::from_millis(100 * 86_400_000);
        let history = [
            worked(1.0, &["a"], 1.0, now),
            worked(1.0, &["b"], 2.0, now),
            worked(1.0, &["c"], 6.0, now),
            worked(1.0, &["d"], 8.0, now),
        ];
        assert!((expected_cores(None, &[], &history, now) - 4.0).abs() < 1e-12);
        let unknown = expected_cores(None, &areas(&["docs"]), &history, now);
        assert!((unknown - 4.0).abs() < 1e-12);
        assert!((expected_cores(None, &[], &[], now) - DEFAULT_CORES).abs() < 1e-12);
        assert!((expected_cores(Some(0.2), &[], &history, now) - 0.2).abs() < 1e-12);
    }

    #[test]
    fn an_area_is_a_package_or_a_second_level_directory() {
        let packages = areas(&["rust/crates/kernel", "rust/crates"]);
        assert_eq!(
            area_of("rust/crates/kernel/src/lib.rs", &packages),
            "rust/crates/kernel"
        );
        assert_eq!(
            area_of("rust/crates/kernelx/src/lib.rs", &packages),
            "rust/crates"
        );
        assert_eq!(area_of("tools/src/by/checks.py", &packages), "tools/src");
        assert_eq!(area_of("tools/x.py", &packages), "tools");
        assert_eq!(area_of("justfile", &packages), "justfile");
        assert_eq!(
            area_of("justfile", &packages),
            crate::inputs::directory_area("justfile")
        );
    }
}
