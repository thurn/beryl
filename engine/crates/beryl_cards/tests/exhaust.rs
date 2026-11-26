//! Bounded exhaustive search from every card scenario's end position, the generated ones'
//! too (architecture §17.4): every sequence of decisions to a fixed depth keeps the engine's
//! invariants and references and plays without a panic. `just exhaust [card] --depth N`
//! searches deeper.
//!
//! `BERYL_EXHAUST_DEPTH` and `BERYL_EXHAUST_NODES` set the bounds, and `BERYL_EXHAUST_CARD`
//! limits the search to one card's scenarios.
//!
//! The same search also starts from positions of random games between decks of the keystone
//! pool (`decks::pool_deck`), where cards of different keystone families are on the battlefield
//! together: `just exhaust --pool` searches them, and `BERYL_EXHAUST_POOL=<first seed>+<games>`
//! says which games.

use std::panic::catch_unwind;

use beryl_cards::decks::pool_deck;
use beryl_cards::{CARDS, names, props};
use beryl_engine::rng::SplitMix64;
use beryl_engine::{
    Choice, DecisionKind, Format, GameTable, Large, Layout, Outcome, Profile, Replay, Scratch,
    Seat, StallReason, State, Step, Stops, TurnStep, apply, next,
};
use beryl_scenario::{
    Action, Bounds, LayoutComparison, Live, P0, P1, Scenario, exhaust, exhaust_position,
    invariants, outcome_with,
};

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
/// ([`beryl_cards::derive`]), and the rules library's scenarios of the shared-deck format by
/// the card that has the effect, so that the search also starts from positions with one shared
/// library and graveyard; or those of the one card `BERYL_EXHAUST_CARD` names.
fn scenarios() -> Vec<(&'static str, &'static Scenario)> {
    let card = var("BERYL_EXHAUST_CARD");
    let mut chosen = Vec::new();
    let wanted = |name: &str| card.as_deref().is_none_or(|c| c.eq_ignore_ascii_case(name));
    for &(name, scenarios) in beryl_cards::scenarios()
        .iter()
        .chain(beryl_cards::derive::all())
    {
        if wanted(name) {
            chosen.extend(scenarios.iter().map(|scenario| (name, scenario)));
        }
    }
    for (name, scenarios) in beryl_cards::library::by_card() {
        if wanted(name) {
            chosen.extend(
                scenarios
                    .iter()
                    .filter(|scenario| scenario.setup.shared.is_some())
                    .map(|scenario| (name, scenario)),
            );
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

/// The scenario runner's profile, which the search plays under: the exact profile, with
/// questions of one answer stopping too, so that a pool game's choices replay as
/// `beryl_search`'s shrinking fuzzer replays a pool game's.
const TRACED: Profile = Profile {
    stops: Stops::Forced,
    ..Profile::EXACT
};

/// The pool games to search from: `BERYL_EXHAUST_POOL`, `<first seed>+<games>`, or two games
/// from seed 0.
fn pool_seeds() -> std::ops::Range<u64> {
    let Some(spec) = var("BERYL_EXHAUST_POOL") else {
        return 0..2;
    };
    let parsed = spec
        .split_once('+')
        .and_then(|(first, n)| Some((first.parse::<u64>().ok()?, n.parse::<u64>().ok()?)));
    let (first, n) =
        parsed.unwrap_or_else(|| panic!("BERYL_EXHAUST_POOL is not <first>+<games>: {spec}"));
    first..first + n
}

/// What one pool game gave the search.
enum Pooled {
    /// It ended, or outgrew `Small`, before a quiet priority of its middle turns.
    NoPosition,
    /// The search from its quiet priority, at the turn given.
    Searched(u16, beryl_scenario::Searched),
}

/// Plays pool game `seed` with seeded random choices to a quiet priority of its middle turns
/// (the active player's, in a main phase, with nothing on the stack, on a turn from the third
/// to the eighth chosen by the seed), with the invariants, the references and the comparison of
/// both layouts at every decision on the way, then searches every sequence within `bounds` from
/// there in `Large`, where no capacity of `Small` can end a branch. A failure names the seed and
/// every choice from the game's start, which `beryl_search`'s `shrink::pool_games_keep_the_invariants`
/// replays as a scenario when given them in `BERYL_POOL_GAME`.
fn pool_game(seed: u64, bounds: Bounds) -> Result<Pooled, String> {
    let decks = [pool_deck(seed), pool_deck(seed.wrapping_add(1 << 32))];
    let t = GameTable::new(
        Format::CONSTRUCTED,
        &CARDS,
        decks.each_ref().map(Vec::as_slice),
    )
    .map_err(|e| format!("pool seed {seed}: {e:?}"))?;
    let mut s: State =
        State::new(&t, seed, TRACED).map_err(|e| format!("pool seed {seed}: {e:?}"))?;
    let mut rng = SplitMix64::new(seed);
    let turn = 3 + u16::try_from(rng.below(6)).expect("a turn");
    let mut comparison = LayoutComparison::new(&s);
    let mut x = Scratch::new();
    let mut choices: Vec<u16> = Vec::new();
    let failed =
        |choices: &[u16], why: String| format!("pool seed {seed}, choices {choices:?}: {why}");
    let played = catch_unwind(std::panic::AssertUnwindSafe(|| {
        loop {
            let before = s;
            let step = next(&mut s, &t, &mut x, &mut Replay);
            // A game that outgrows `Small` goes on in `Large` in the app; the family fuzzers
            // play that on, and this search starts only from games still in `Small`.
            if s.outcome() == Some(Outcome::Stalled(StallReason::Capacity)) {
                return Ok(Pooled::NoPosition);
            }
            invariants(&t, &s).map_err(|why| failed(&choices, why))?;
            comparison
                .check_next(&t, &s, &step)
                .map_err(|why| failed(&choices, why))?;
            let d = match step {
                Step::Decide(d) => d,
                Step::Over(_) => return Ok(Pooled::NoPosition),
            };
            if d.is_empty() {
                return Err(failed(&choices, format!("no candidates for {d:?}")));
            }
            let at = s.turn();
            let quiet = d.kind == DecisionKind::Priority
                && d.player == at.active()
                && s.stack().is_empty()
                && matches!(
                    at.step(),
                    TurnStep::PrecombatMain | TurnStep::PostcombatMain
                )
                && at.number() >= turn;
            if quiet {
                let mut large: State<Large> = before.widen();
                let mut xl = Scratch::new();
                let Step::Decide(first) = next(&mut large, &t, &mut xl, &mut Replay) else {
                    return Err(failed(&choices, "the widened game ended".to_owned()));
                };
                return exhaust_position(&t, &large, &first, bounds, invariants)
                    .map(|searched| Pooled::Searched(at.number(), searched))
                    .map_err(|found| {
                        let mut all = choices.clone();
                        all.extend(&found.choices);
                        format!(
                            "pool seed {seed}, turn {}, choices {all:?}: {}\n{}",
                            at.number(),
                            found.why,
                            found.text
                        )
                    });
            }
            let c = Choice(u16::try_from(rng.below(d.len() as u64)).expect("a candidate"));
            choices.push(c.0);
            comparison.apply(&t, c);
            apply(&mut s, &t, &mut x, &mut Replay, c);
        }
    }));
    played.unwrap_or_else(|panic| {
        let why = panic
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| panic.downcast_ref::<&str>().map(|&m| m.to_owned()))
            .unwrap_or_else(|| "a panic".to_owned());
        Err(format!("pool seed {seed}: {why}"))
    })
}

/// Every sequence of decisions to a depth from positions of pool games, where cards of
/// different keystone families meet, keeps the invariants and plays without a panic.
#[test]
#[ignore = "searches from random games of the keystone pool: `just exhaust --pool` runs it"]
fn pool_positions_keep_the_invariants_to_a_depth() {
    let bounds = bounds();
    let seeds: Vec<u64> = pool_seeds().collect();
    let threads = std::thread::available_parallelism().map_or(4, usize::from);
    let chunk = seeds.len().div_ceil(threads).max(1);
    let results: Vec<(u64, Result<Pooled, String>)> = std::thread::scope(|scope| {
        let handles: Vec<_> = seeds
            .chunks(chunk)
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|&seed| (seed, pool_game(seed, bounds)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("a pool game's thread"))
            .collect()
    });
    let (mut searched, mut nodes, mut capped, mut skipped) = (0, 0, 0, 0);
    let mut failures = Vec::new();
    for (seed, result) in results {
        match result {
            Ok(Pooled::Searched(turn, s)) => {
                println!(
                    "  pool seed {seed}: turn {turn}, {} decisions{}",
                    s.nodes,
                    if s.capped { ", capped" } else { "" }
                );
                searched += 1;
                nodes += s.nodes;
                capped += usize::from(s.capped);
            }
            Ok(Pooled::NoPosition) => skipped += 1,
            Err(why) => failures.push(why),
        }
    }
    println!(
        "searched {searched} pool positions to depth {}: {nodes} decisions; {capped} capped at {} decisions; {skipped} games reached no position",
        bounds.depth, bounds.nodes
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
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
