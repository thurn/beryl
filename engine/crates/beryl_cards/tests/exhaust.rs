//! Bounded exhaustive search from every card scenario's end position, the generated ones'
//! too (architecture §17.4): every sequence of decisions to a fixed depth keeps the engine's
//! invariants and references and plays without a panic. `just exhaust [card] --depth N`
//! searches deeper.
//!
//! `BERYL_EXHAUST_DEPTH` and `BERYL_EXHAUST_NODES` set the bounds, and `BERYL_EXHAUST_CARD`
//! limits the search to one card's scenarios.

use std::panic::catch_unwind;

use beryl_cards::{CARDS, names, props};
use beryl_engine::{GameTable, Large, Layout, Seat, State};
use beryl_scenario::{Action, Bounds, Live, P0, P1, Scenario, exhaust, invariants, outcome_with};

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
    let mut chosen = Vec::new();
    for &(name, scenarios) in beryl_cards::scenarios()
        .iter()
        .chain(beryl_cards::derive::all())
    {
        if card.as_deref().is_none_or(|c| c.eq_ignore_ascii_case(name)) {
            chosen.extend(scenarios.iter().map(|scenario| (name, scenario)));
        }
    }
    assert!(!chosen.is_empty(), "no card named {card:?}");
    chosen
}

/// Checks the invariants from each scenario, prints what it covered, and returns the failures.
fn search(scenarios: &[(&str, &'static Scenario)], bounds: Bounds) -> Vec<String> {
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
                            (
                                card,
                                scenario,
                                exhaust(&CARDS, scenario, bounds, invariants, invariants),
                            )
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
    let failures = search(&scenarios(), bounds);
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
/// spell that resolves immediately would replay to an empty stack. The opponent has mana to
/// cast Counterspell, so the runner stops with the first spell still on the stack.
#[test]
fn a_broken_invariant_is_found_and_replays() {
    #[allow(clippy::unnecessary_wraps)] // A `Check`, failing by panicking.
    fn stack_stays_empty<L: Layout>(_: &GameTable, s: &State<L>) -> Result<(), String> {
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
    const SCENARIO: Scenario = Scenario::new("A spell the opponent can counter")
        .battlefield(P0, &[names::PLAINS])
        .hand(P0, &[props::ARTIFACT.card])
        .battlefield(P1, &[names::ISLAND; 2])
        .hand(P1, &[names::COUNTERSPELL]);
    let found = exhaust(
        &CARDS,
        &SCENARIO,
        BOUNDS,
        stack_stays_empty,
        stack_stays_empty,
    )
    .expect_err("casting the spell breaks the invariant");
    assert!(found.searched > 0, "{found}");
    assert!(found.why.contains("something is on the stack"), "{found}");
    assert!(breaks(
        found.scenario.as_ref().expect("a scriptable sequence")
    ));
    println!("{found}");
}

#[test]
fn a_counterexample_replays_inside_a_subgame() {
    const SCENARIO: Scenario = Scenario::new("Players choose opening hands in a subgame")
        .hand(P0, &[names::SHAHRAZAD])
        .battlefield(P0, &[names::PLAINS; 2])
        .library(P0, &[names::PLAINS; 8])
        .library(P1, &[names::ISLAND; 8])
        .actions(&[Action::cast(P0, names::SHAHRAZAD), Action::Resolve]);

    fn mulliganed_in_subgame(state: &State<Large>) -> bool {
        state.subgame_depth() == 1
            && Seat::BOTH
                .into_iter()
                .any(|seat| state.player(seat).mulligans() > 0)
    }
    let found = exhaust(&CARDS, &SCENARIO, BOUNDS, invariants, |_, state| {
        if mulliganed_in_subgame(state) {
            return Err("a player mulliganed in the subgame".to_owned());
        }
        Ok(())
    })
    .expect_err("the search should find a mulligan in the subgame");
    assert_eq!(found.why, "a player mulliganed in the subgame");
    let mut replayed_violation = false;
    outcome_with(
        &CARDS,
        found.scenario.as_ref().expect("a scriptable sequence"),
        |_, _| panic!("a subgame scenario must use Large"),
        |state, _| replayed_violation = mulliganed_in_subgame(state),
    )
    .expect("the counterexample should replay");
    assert!(replayed_violation, "{found}");
}
