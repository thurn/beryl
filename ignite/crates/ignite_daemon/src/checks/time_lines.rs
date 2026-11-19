//! The lines of `ignite test`, `ignite check` and `ignite land` that say where time went apart
//! from running checks: a `built` line per product built, and the `listed` line for the
//! families whose `list` command ran.

use std::collections::BTreeMap;

use super::{Built, CheckRun, ListingTime, plan};

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
