//! Bounded exhaustive search from every card scenario's end position, the generated ones'
//! too (architecture §17.4): every sequence of decisions to a fixed depth keeps the engine's
//! invariants and references and plays without a panic. `just exhaust [card] --depth N`
//! searches deeper.
//!
//! `BERYL_EXHAUST_DEPTH` and `BERYL_EXHAUST_NODES` set the bounds, and `BERYL_EXHAUST_CARD`
//! limits the search to one card's scenarios.

use std::panic::catch_unwind;

use beryl_cards::CARDS;
use beryl_engine::{GameTable, State};
use beryl_scenario::{Action, Bounds, Check, Live, Scenario, exhaust, invariants};

/// The default bounds: the whole corpus in a few seconds of a debug build.
const BOUNDS: Bounds = Bounds {
    depth: 3,
    nodes: 400,
};

/// Scenarios whose search finds a known rules bug too large to fix where it was found, each
/// with the bead that fixes it. The test prints them, and fails once one passes, so the list
/// stays current.
const SKIPPED: &[(&str, &str)] = &[];

/// The variable `name`, if it is set and not empty.
fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn number<T: std::str::FromStr>(name: &str) -> Option<T> {
    var(name).map(|v| {
        v.parse()
            .unwrap_or_else(|_| panic!("{name} is not a number: {v}"))
    })
}

fn bounds() -> Bounds {
    Bounds {
        depth: number("BERYL_EXHAUST_DEPTH").unwrap_or(BOUNDS.depth),
        nodes: number("BERYL_EXHAUST_NODES").unwrap_or(BOUNDS.nodes),
    }
}

/// The scenarios to search: every card's, hand-written and generated
/// ([`beryl_cards::derive`]), or the one card's `BERYL_EXHAUST_CARD` names.
fn scenarios() -> Vec<(&'static str, &'static Scenario)> {
    let card = var("BERYL_EXHAUST_CARD");
    let chosen: Vec<_> = beryl_cards::scenarios()
        .iter()
        .chain(beryl_cards::derive::all())
        .filter(|&&(name, _)| card.as_deref().is_none_or(|c| c.eq_ignore_ascii_case(name)))
        .flat_map(|&(name, scenarios)| scenarios.iter().map(move |s| (name, s)))
        .collect();
    assert!(!chosen.is_empty(), "no card named {card:?}");
    chosen
}

/// Searches `scenarios` under `check`, printing what it covered, and returns the failures.
fn search(scenarios: &[(&str, &'static Scenario)], bounds: Bounds, check: Check) -> Vec<String> {
    let (mut nodes, mut capped, mut failures) = (0, Vec::new(), Vec::new());
    // The scenarios shared among threads, the results kept in their order.
    let threads = std::thread::available_parallelism().map_or(4, usize::from);
    let chunk = scenarios.len().div_ceil(threads).max(1);
    let results: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = scenarios
            .chunks(chunk)
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|&(card, scenario)| {
                            (card, scenario, exhaust(&CARDS, scenario, bounds, check))
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("a search panicked"))
            .collect()
    });
    for (card, scenario, result) in results {
        match result {
            Ok(searched) => {
                nodes += searched.nodes;
                if searched.capped {
                    capped.push(format!("{card}: {}", scenario.name));
                }
            }
            Err(found) => failures.push(format!("{card}:\n{found}")),
        }
    }
    println!(
        "searched {} scenarios to depth {}: {nodes} decisions; {} capped at {} decisions",
        scenarios.len(),
        bounds.depth,
        capped.len(),
        bounds.nodes,
    );
    for c in &capped {
        println!("  capped: {c}");
    }
    failures
}

#[test]
fn every_scenario_keeps_the_invariants_to_a_depth() {
    let bounds = bounds();
    let failures = search(&scenarios(), bounds, invariants);
    let mut unexpected = Vec::new();
    for failure in &failures {
        if SKIPPED
            .iter()
            .any(|(name, _)| failure.contains(&format!("// {name}:")))
        {
            println!("skipped, a known bug:\n{failure}");
        } else {
            unexpected.push(failure.as_str());
        }
    }
    for (name, bead) in SKIPPED {
        println!("skipping {name} ({bead})");
        assert!(
            failures.iter().any(|f| f.contains(&format!("// {name}:"))),
            "{name} passes now: take it off the skip list"
        );
    }
    assert!(unexpected.is_empty(), "{}", unexpected.join("\n"));
}

/// A deliberately broken invariant, that the stack stays empty, is found from a scenario's end
/// position, and its transcript replays to a position that breaks it. The check panics, as the
/// engine's debug assertions do, rather than returning a violation.
///
/// The search checks every position, the runner stops only where someone has a choice, and a
/// spell or ability that no one can respond to resolves between the two: a search that breaks
/// the check there replays to an empty stack. So the test takes the first finding the runner
/// can stop at, and there must be one.
#[test]
fn a_broken_invariant_is_found_and_replays() {
    #[allow(clippy::unnecessary_wraps)] // A `Check`, failing by panicking.
    fn stack_stays_empty(_: &GameTable, s: &State) -> Result<(), String> {
        assert!(s.stack().is_empty(), "something is on the stack");
        Ok(())
    }
    /// Whether `scenario`, a finding's transcript, plays to a position that breaks the check.
    fn breaks(scenario: &Scenario) -> bool {
        let mut live = Live::new(&CARDS, scenario).unwrap();
        for &action in scenario.actions {
            live.act(action)
                .unwrap_or_else(|f| panic!("{f}\n{}", scenario.name));
        }
        live.position(Action::pass(scenario.setup.active))
            .unwrap_or_else(|f| panic!("{f}\n{}", scenario.name));
        catch_unwind(|| stack_stays_empty(live.table(), live.state())).is_err()
    }
    let found = scenarios()
        .into_iter()
        .find_map(|(_, scenario)| {
            exhaust(&CARDS, scenario, BOUNDS, stack_stays_empty)
                .err()
                .filter(|found| found.searched > 0)
                .filter(|found| {
                    assert!(found.why.contains("something is on the stack"), "{found}");
                    breaks(found.scenario.as_ref().expect("a scriptable sequence"))
                })
        })
        .expect("a scenario's search breaks the check where the runner stops");
    println!("{found}");
}
