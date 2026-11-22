//! The lines of `ignite test`, `ignite check` and `ignite land` that say where time went apart
//! from running checks: a `built` line per product built, the `listed` line for the families
//! whose `list` command ran, the `took` line that splits the command's time into its stages,
//! and the `slow` line that names every other step of five seconds or more.
//!
//! A step that is neither a build, a listing nor a check run, such as hashing a large family's
//! inputs or waiting for a build another command started, was once reported nowhere: on
//! 2026-11-19 `ignite test` of a one-line card change took 300 s, and its lines accounted for
//! 130 s of them.

use std::collections::BTreeMap;

use ignite_core::time::Timestamp;

use super::{Built, CheckRun, ListingTime, plan};
use crate::server::Context;

/// The shortest step that [`CheckRun::slow`] keeps, in seconds.
const KEPT_S: f64 = 1.0;

/// The shortest step the `slow` line names, in seconds.
const SHOWN_S: f64 = 5.0;

/// When a step started, read from the context's clock.
#[derive(Clone, Copy)]
pub(crate) struct Stopwatch(Timestamp);

impl Stopwatch {
    pub(crate) fn start(ctx: &Context) -> Self {
        Self(ctx.clock.now())
    }

    /// Seconds since the step started.
    pub(crate) fn seconds(self, ctx: &Context) -> f64 {
        ctx.clock.now().seconds_since(self.0).max(0.0)
    }
}

impl CheckRun {
    /// Records one of the command's stages, which run one after another, as having taken the
    /// seconds since `started`.
    pub(crate) fn stage(&mut self, ctx: &Context, what: &str, started: Stopwatch) {
        self.stages.push((what.to_owned(), started.seconds(ctx)));
    }

    /// Records a step inside a stage as having taken the seconds since `started`, if it took at
    /// least a second; the `slow` line names the steps of [`SHOWN_S`] or more.
    pub(crate) fn slow(&mut self, ctx: &Context, what: impl Into<String>, started: Stopwatch) {
        let seconds = started.seconds(ctx);
        if seconds >= KEPT_S {
            self.slow_steps.push((what.into(), seconds));
        }
    }

    /// Adds `other`, what one family's work found on a thread of its own, after what this run
    /// holds. A build failure already recorded is not recorded again.
    pub(crate) fn absorb(&mut self, other: CheckRun) {
        // Taken apart field by field, so a field added to `CheckRun` cannot be forgotten here.
        let CheckRun {
            outcomes,
            measurement_hashes,
            from_cache,
            ran,
            failures,
            builds,
            build_failures,
            listing_build_failures,
            incomplete_families,
            out_of_scope,
            skipped,
            unlisted,
            cold_families,
            wall_s,
            listings,
            listings_kept,
            records,
            stages,
            slow_steps,
        } = other;
        self.outcomes.extend(outcomes);
        self.measurement_hashes.extend(measurement_hashes);
        self.from_cache += from_cache;
        self.ran += ran;
        self.failures.extend(failures);
        self.builds.extend(builds);
        for (product, output) in build_failures {
            if !self.build_failures.iter().any(|(p, _)| *p == product) {
                self.build_failures.push((product, output));
            }
        }
        self.listing_build_failures.extend(listing_build_failures);
        self.incomplete_families.extend(incomplete_families);
        self.out_of_scope.extend(out_of_scope);
        self.skipped += skipped;
        self.unlisted.extend(unlisted);
        self.cold_families.extend(cold_families);
        self.wall_s += wall_s;
        self.listings.extend(listings);
        self.listings_kept += listings_kept;
        self.records.extend(records);
        self.stages.extend(stages);
        self.slow_steps.extend(slow_steps);
    }
}

/// The `took` line: the command's seconds, `total`, split into its stages, with what no stage
/// covers as `other`. `None` for a command quicker than [`SHOWN_S`], which needs no account.
///
/// `took     301 s: opening the tree 3 s, finding families 14 s, listing 130 s, running 154 s`
pub(crate) fn took_line(run: &CheckRun, total: f64) -> Option<(String, String)> {
    if total < SHOWN_S {
        return None;
    }
    let mut parts: Vec<String> = run
        .stages
        .iter()
        .filter(|(_, seconds)| *seconds >= KEPT_S)
        .map(|(what, seconds)| format!("{what} {seconds:.0} s"))
        .collect();
    let covered: f64 = run.stages.iter().map(|(_, seconds)| seconds).sum();
    let other = total - covered;
    if other >= KEPT_S {
        parts.push(format!("other {other:.0} s"));
    }
    let text = if parts.is_empty() {
        format!("{total:.0} s")
    } else {
        format!("{total:.0} s: {}", parts.join(", "))
    };
    Some(("took".to_owned(), text))
}

/// The `slow` line: every step of [`SHOWN_S`] seconds or more that no `built` or `listed` line
/// names, longest first, or `None` when there is none. Steps on threads of their own may
/// overlap, so their seconds can add up to more than the command took. A step that several
/// families took, such as waiting for the one build they all need, is named once, with its
/// longest time and how many times it was taken.
///
/// `slow     waiting for the build of b3d 92 s, 3 times; keying b3d-every-tick 25 s`
pub(crate) fn slow_line(run: &CheckRun) -> Option<(String, String)> {
    let mut steps: BTreeMap<&str, (f64, usize)> = BTreeMap::new();
    for (what, seconds) in run.slow_steps.iter().filter(|(_, s)| *s >= SHOWN_S) {
        let step = steps.entry(what.as_str()).or_insert((0.0, 0));
        step.0 = step.0.max(*seconds);
        step.1 += 1;
    }
    if steps.is_empty() {
        return None;
    }
    let mut slow: Vec<(&str, (f64, usize))> = steps.into_iter().collect();
    slow.sort_by(|a, b| b.1.0.total_cmp(&a.1.0));
    let named: Vec<String> = slow
        .iter()
        .map(|(what, (seconds, times))| match times {
            1 => format!("{what} {seconds:.0} s"),
            _ => format!("{what} {seconds:.0} s, {times} times"),
        })
        .collect();
    Some(("slow".to_owned(), named.join("; ")))
}

/// How many families the `listed` line names.
const SLOWEST: usize = 3;

/// How many crates a `built` line names as the costliest it compiled.
pub(crate) const COSTLIEST: usize = 3;

pub(crate) fn built_lines(run: &CheckRun) -> Vec<(String, String)> {
    run.builds
        .iter()
        .map(|b| ("built".to_owned(), built_line(b)))
        .collect()
}

/// `ignite-tests in 319 s, 191 CPU-s, 0.6 cores busy (22 crates recompiled; most CPU:
/// ignite_daemon 80 s, ignite_core 31 s, rusqlite 12 s)`: the wall time, the CPU seconds and
/// the cores they kept busy on average, the seconds spent queued for cores when there were any,
/// how many crates were compiled and the [`COSTLIEST`] of them by CPU seconds.
fn built_line(b: &Built) -> String {
    let running = (b.wall_s - b.queued_s).max(b.wall_s.min(1.0)).max(0.001);
    let queued = if b.queued_s >= 1.0 {
        format!(" ({:.0} s queued)", b.queued_s)
    } else {
        String::new()
    };
    let crates = match b.rebuilt {
        Some(1) => "1 crate recompiled".to_owned(),
        Some(n) => format!("{n} crates recompiled"),
        None => "crates recompiled unknown".to_owned(),
    };
    let named: Vec<String> = b
        .costliest
        .iter()
        .map(|(name, cpu_s)| format!("{name} {cpu_s:.0} s"))
        .collect();
    let crates = if named.is_empty() {
        crates
    } else {
        format!("{crates}; most CPU: {}", named.join(", "))
    };
    format!(
        "{} in {:.0} s{queued}, {:.0} CPU-s, {:.1} cores busy ({crates})",
        b.product,
        b.wall_s,
        b.cpu_s,
        b.cpu_s / running,
    )
}

/// The `listed` line: the seconds spent running `list` commands, how many families were listed
/// afresh and how many came from kept listings, and the slowest families, each with what its
/// listing waited for: Cargo's lock, or compiling. `None` when no family was listed either way.
///
/// `listed   1,003 s listing 4 families afresh, 12 kept; slowest: rust 640 s (600 s waiting
/// for Cargo's lock; 31 crates compiled in 38 s), iroh 300 s (212 crates compiled in 290 s),
/// ignite 19 s`
pub(crate) fn listed_line(run: &CheckRun) -> Option<(String, String)> {
    let kept = run.listings_kept;
    if run.listings.is_empty() && kept == 0 {
        return None;
    }
    let mut by_family: BTreeMap<&str, ListingTime> = BTreeMap::new();
    for listing in &run.listings {
        let total = by_family
            .entry(listing.family.as_str())
            .or_insert_with(|| ListingTime {
                family: listing.family.clone(),
                ..ListingTime::default()
            });
        total.wall_s += listing.wall_s;
        total.compiled += listing.compiled;
        total.compile_s += listing.compile_s;
        total.lock_s += listing.lock_s;
    }
    let fresh = by_family.len();
    if fresh == 0 {
        return Some((
            "listed".to_owned(),
            format!("nothing listed afresh, {kept} kept"),
        ));
    }
    let seconds: f64 = by_family.values().map(|t| t.wall_s).sum();
    let mut slowest: Vec<&ListingTime> = by_family.values().collect();
    slowest.sort_by(|a, b| b.wall_s.total_cmp(&a.wall_s));
    let named: Vec<String> = slowest
        .iter()
        .take(SLOWEST)
        .map(|t| family_text(t))
        .collect();
    let word = if fresh == 1 { "family" } else { "families" };
    Some((
        "listed".to_owned(),
        format!(
            "{} s listing {fresh} {word} afresh, {kept} kept; slowest: {}",
            plan::thousands(seconds.round() as usize),
            named.join(", ")
        ),
    ))
}

/// `rust 640 s (600 s waiting for Cargo's lock; 31 crates compiled in 38 s)`: one family's
/// listing seconds, and what of them went to Cargo's lock and to compiling when either did.
fn family_text(time: &ListingTime) -> String {
    let mut parts = Vec::new();
    if time.lock_s >= 1.0 {
        parts.push(format!("{:.0} s waiting for Cargo's lock", time.lock_s));
    }
    if time.compiled > 0 {
        let word = if time.compiled == 1 {
            "crate"
        } else {
            "crates"
        };
        parts.push(format!(
            "{} {word} compiled in {:.0} s",
            time.compiled, time.compile_s
        ));
    }
    let why = if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join("; "))
    };
    format!("{} {:.0} s{why}", time.family, time.wall_s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn built(rebuilt: Option<usize>) -> Built {
        Built {
            product: "ignite-tests".to_owned(),
            wall_s: 319.0,
            rebuilt,
            costliest: Vec::new(),
            cpu_s: 191.0,
            queued_s: 19.0,
        }
    }

    #[test]
    fn the_line_shows_crates_cpu_seconds_cores_and_queueing() {
        assert_eq!(
            built_line(&built(Some(22))),
            "ignite-tests in 319 s (19 s queued), 191 CPU-s, 0.6 cores busy (22 crates recompiled)"
        );
        assert!(built_line(&built(Some(1))).ends_with("(1 crate recompiled)"));
    }

    #[test]
    fn the_line_names_the_costliest_crates() {
        let mut b = built(Some(85));
        b.costliest = vec![("bevy_render".to_owned(), 41.4), ("cards".to_owned(), 9.6)];
        assert!(
            built_line(&b)
                .ends_with("(85 crates recompiled; most CPU: bevy_render 41 s, cards 10 s)"),
            "{}",
            built_line(&b)
        );
    }

    #[test]
    fn a_build_with_no_recorded_compiles_says_the_count_is_unknown() {
        assert!(built_line(&built(None)).ends_with("(crates recompiled unknown)"));
    }

    fn listing(
        family: &str,
        wall_s: f64,
        compiled: usize,
        compile_s: f64,
        lock_s: f64,
    ) -> ListingTime {
        ListingTime {
            family: family.to_owned(),
            wall_s,
            compiled,
            compile_s,
            lock_s,
        }
    }

    #[test]
    fn the_listed_line_names_the_slowest_families_and_what_they_waited_for() {
        let run = CheckRun {
            listings: vec![
                listing("ignite", 19.0, 0, 0.0, 0.0),
                listing("rust", 640.0, 31, 38.0, 600.0),
                listing("script", 9.0, 0, 0.0, 0.0),
                listing("iroh", 300.0, 212, 290.0, 0.0),
            ],
            listings_kept: 12,
            ..CheckRun::default()
        };
        let (label, text) = listed_line(&run).unwrap();
        assert_eq!(label, "listed");
        assert_eq!(
            text,
            "968 s listing 4 families afresh, 12 kept; slowest: rust 640 s (600 s waiting for \
             Cargo's lock; 31 crates compiled in 38 s), iroh 300 s (212 crates compiled in 290 \
             s), ignite 19 s"
        );
    }

    #[test]
    fn a_family_listed_twice_counts_once_with_both_listings() {
        let run = CheckRun {
            listings: vec![
                listing("rust", 10.0, 1, 5.0, 0.0),
                listing("rust", 20.0, 2, 8.0, 0.0),
            ],
            ..CheckRun::default()
        };
        let (_, text) = listed_line(&run).unwrap();
        assert_eq!(
            text,
            "30 s listing 1 family afresh, 0 kept; slowest: rust 30 s (3 crates compiled in 13 s)"
        );
    }

    #[test]
    fn the_took_line_splits_the_time_into_stages_and_names_the_rest_other() {
        let run = CheckRun {
            stages: vec![
                ("opening the tree".to_owned(), 0.4),
                ("finding families".to_owned(), 14.2),
                ("listing".to_owned(), 130.0),
                ("running".to_owned(), 150.0),
            ],
            ..CheckRun::default()
        };
        assert_eq!(
            took_line(&run, 301.0),
            Some((
                "took".to_owned(),
                "301 s: finding families 14 s, listing 130 s, running 150 s, other 6 s".to_owned()
            ))
        );
        assert_eq!(took_line(&run, 4.0), None);
    }

    #[test]
    fn the_slow_line_names_each_step_of_five_seconds_or_more_once_longest_first() {
        let mut run = CheckRun {
            slow_steps: vec![
                ("keying b3d-every-tick".to_owned(), 25.0),
                ("reading kept listings".to_owned(), 2.0),
                ("waiting for the build of b3d".to_owned(), 92.0),
                ("waiting for the build of b3d".to_owned(), 90.0),
            ],
            ..CheckRun::default()
        };
        assert_eq!(
            slow_line(&run).unwrap().1,
            "waiting for the build of b3d 92 s, 2 times; keying b3d-every-tick 25 s"
        );
        run.slow_steps.clear();
        assert_eq!(slow_line(&run), None);
    }

    #[test]
    fn absorbing_a_run_adds_its_findings_and_reports_a_failed_build_once() {
        let output = std::path::PathBuf::from("out");
        let mut run = CheckRun {
            from_cache: 1,
            build_failures: vec![("b3d".to_owned(), output.clone())],
            ..CheckRun::default()
        };
        run.absorb(CheckRun {
            from_cache: 2,
            build_failures: vec![("b3d".to_owned(), output)],
            listings: vec![listing("rust", 3.0, 0, 0.0, 0.0)],
            slow_steps: vec![("keying rust".to_owned(), 6.0)],
            ..CheckRun::default()
        });
        assert_eq!(run.from_cache, 3);
        assert_eq!(run.build_failures.len(), 1);
        assert_eq!(run.listings.len(), 1);
        assert_eq!(run.slow_steps.len(), 1);
    }

    #[test]
    fn only_kept_listings_say_so_and_no_listing_prints_nothing() {
        let run = CheckRun {
            listings_kept: 5,
            ..CheckRun::default()
        };
        assert_eq!(
            listed_line(&run).unwrap().1,
            "nothing listed afresh, 5 kept"
        );
        assert_eq!(listed_line(&CheckRun::default()), None);
    }
}
