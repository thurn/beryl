//! Fuzz games that shrink (plan K12). A game is a seed, which deals two fuzz decks and draws
//! the game's randomness, and the choices taken at its decisions, each an index taken modulo
//! the decision's candidate count; it plays until its choices run out or it ends. proptest
//! generates the choices, and when a game breaks the property (an invariant or a reference
//! violated, a capacity run out, a panic) it shrinks them to the fewest that still break it
//! and the test prints that game as a scenario to paste into a card's file.
//! `BERYL_FUZZ_GAMES=<n>` plays more games, as `just fuzz` does.
//!
//! Games are played under the exact profile with forced questions stopping too, as the
//! scenario runner plays them, so that the printed scenario takes the same decisions. The
//! smart profile and the large layout are fuzzed by `games.rs`.

mod common;

use std::fmt::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};

use beryl_cards::CARDS;
use beryl_cards::decks::fuzz_deck;
use beryl_engine::{
    Choice, Decision, DecisionKind, GameTable, Outcome, Profile, Replay, Scratch, Seat,
    StallReason, State, Step, Stops, Zone, ZoneKey, apply, next,
};
use beryl_scenario::{
    Check, Live, Scenario, answer, describe, invariants, script, setup_source, source,
};
use common::games;
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::test_runner::{Config, RngAlgorithm, TestError, TestRng, TestRunner};

/// The scenario runner's profile: the exact profile, with forced questions stopping too.
const TRACED: Profile = Profile {
    stops: Stops::Forced,
    ..Profile::EXACT
};

/// The most choices a game is generated with: past most random games' ends.
const MOST_CHOICES: usize = 600;

/// A fuzz game: its seed and the choice at each decision, by index modulo the candidates.
#[derive(Clone, Debug)]
struct Game {
    seed: u64,
    choices: Vec<u16>,
}

impl Game {
    fn table(&self) -> GameTable {
        let decks = [
            fuzz_deck(self.seed),
            fuzz_deck(self.seed.wrapping_add(1 << 32)),
        ];
        GameTable::new(&CARDS, [decks[0].as_slice(), decks[1].as_slice()]).unwrap()
    }

    /// Plays the game, calling `visit` at every decision with the choice it takes, and checks
    /// `check` at every position from the first decision on, the one after the last choice
    /// included. A panic is a failure like any other, so that a replay of a game that panics
    /// still visits each decision before it.
    fn play(
        &self,
        check: Check,
        mut visit: impl FnMut(&GameTable, &State, &Decision, Choice),
    ) -> Result<(), String> {
        let mut i = 0;
        catch_unwind(AssertUnwindSafe(|| {
            let t = self.table();
            let mut s = State::new(&t, self.seed, TRACED);
            let mut x = Scratch::new();
            loop {
                let step = next(&mut s, &t, &mut x, &mut Replay);
                check(&t, &s)?;
                let d = match step {
                    Step::Decide(d) => d,
                    Step::Over(Outcome::Stalled(StallReason::Capacity)) => {
                        return Err("a capacity ran out".to_owned());
                    }
                    Step::Over(_) => return Ok(()),
                };
                if d.is_empty() {
                    return Err(format!("no candidates for {d:?}"));
                }
                let Some(&n) = self.choices.get(i) else {
                    return Ok(());
                };
                let c = Choice(u16::try_from(usize::from(n) % d.len()).unwrap());
                visit(&t, &s, &d, c);
                apply(&mut s, &t, &mut x, &mut Replay, c);
                i += 1;
            }
        }))
        .unwrap_or_else(|panic| {
            Err(panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|&m| m.to_owned()))
                .map_or_else(|| "a panic".to_owned(), |m| format!("a panic: {m}")))
        })
        .map_err(|why| format!("after {i} choices: {why}"))
    }

    /// The game with each choice as the index it took, up to where it stops.
    fn taken(&self, check: Check) -> Game {
        let mut choices = Vec::new();
        let _ = self.play(check, |_, _, _, c| choices.push(c.0));
        Game {
            seed: self.seed,
            choices,
        }
    }

    /// The game as a scenario: from its first priority decision, where the active player
    /// holds priority with nothing on the stack, each hand and library as the game dealt them
    /// and every choice after as an action. What came before (the play-or-draw and the
    /// mulligans) is in comments above it, and choices the vocabulary cannot name in
    /// comments among the actions. Its seed draws the randomness after the setup anew, so
    /// a game that shuffles or flips a coin past its first priority may go another way.
    /// Returns the source and, when the game reached a first priority, the scenario.
    fn scenario(&self, check: Check, why: &str) -> (String, Option<Scenario>) {
        let mut opening = String::new();
        let mut setup: Option<Scenario> = None;
        let (mut actions, mut lines) = (Vec::new(), Vec::new());
        let _ = self.play(check, |t, s, d, c| {
            let taken = match (d.candidates.object(c), d.candidates.option(c)) {
                (Some(slot), _) => s.name(t, slot).to_owned(),
                (_, o) => format!("{o:?}"),
            };
            let taken = format!("{} -> {taken}", describe(t, s, d));
            if setup.is_none() {
                let turn = s.turn();
                if d.kind != DecisionKind::Priority
                    || d.player != turn.active
                    || !s.stack().is_empty()
                {
                    let _ = writeln!(opening, "// {taken}");
                    return;
                }
                setup = Some(position(t, s, self.seed));
            }
            match script(t, s, d, c) {
                Some(action) if answer(t, s, d, &action) == Some(c) => {
                    lines.push(format!("        {},", source(&action)));
                    actions.push(action);
                }
                _ => lines.push(format!("        // not scriptable: {taken}")),
            }
        });
        let Some(scenario) = setup else {
            return (
                format!("{opening}// (no priority decision reached)\n"),
                None,
            );
        };
        let scenario = Scenario {
            name: why.to_owned().leak(),
            ..scenario.actions(actions.leak())
        };
        let mut text = opening;
        text.push_str(&setup_source(&scenario));
        let _ = writeln!(text, "    .actions(&[\n{}\n    ])", lines.join("\n"));
        (text, Some(scenario))
    }
}

/// The setup of a scenario at position `s` of a game's first priority: each player's life,
/// hand and library, top first. Nothing else is anywhere yet.
fn position(t: &GameTable, s: &State, seed: u64) -> Scenario {
    let names = |p, zone| -> &'static [&'static str] {
        // The names of a hand, which has no order.
        s.in_candidate_order(s.zone(ZoneKey::Of(p, zone)))
            .map(|slot| s.name(t, slot))
            .collect::<Vec<_>>()
            .leak()
    };
    assert!(
        s.battlefield().is_empty()
            && Seat::BOTH.iter().all(|&p| {
                s.zone(ZoneKey::Of(p, Zone::Graveyard)).is_empty()
                    && s.zone(ZoneKey::Of(p, Zone::Exile)).is_empty()
            }),
        "the first priority comes before anything is played"
    );
    let turn = s.turn();
    let mut scenario = Scenario::new("")
        .seed(seed)
        .at(turn.number, turn.active, turn.step);
    for p in Seat::BOTH {
        // The state lists a library bottom first.
        let library: Vec<&'static str> = s
            .library(p)
            .iter()
            .rev()
            .map(|&slot| s.name(t, slot))
            .collect();
        scenario = scenario
            .life(p, s.player(p).life)
            .hand(p, names(p, Zone::Hand))
            .library(p, library.leak());
    }
    scenario
}

/// Runs `cases` fuzz games under `check`, deterministically, and returns the game proptest
/// shrank a failure to, with why it fails.
fn shrink(cases: u32, check: Check) -> Option<(Game, String)> {
    let config = Config {
        cases,
        failure_persistence: None,
        max_shrink_iters: 100_000,
        ..Config::default()
    };
    let mut runner =
        TestRunner::new_with_rng(config, TestRng::deterministic_rng(RngAlgorithm::ChaCha));
    let games = (0..u64::from(u32::MAX), vec(any::<u16>(), 0..MOST_CHOICES))
        .prop_map(|(seed, choices)| Game { seed, choices });
    match runner.run(&games, |game| {
        game.play(check, |_, _, _, _| {})
            .map_err(TestCaseError::fail)
    }) {
        Ok(()) => None,
        Err(TestError::Fail(why, game)) => Some((game.taken(check), why.to_string())),
        Err(TestError::Abort(why)) => panic!("fuzzing stopped: {why}"),
    }
}

/// Every prefix of every fuzz game keeps the invariants and plays without a panic.
#[test]
fn fuzz_games_keep_the_invariants() {
    let cases = u32::try_from(games(12)).expect("a game count");
    if let Some((game, why)) = shrink(cases, invariants) {
        let (text, _) = game.scenario(invariants, &why);
        panic!(
            "seed {}, choices {:?}: {why}\n{text}",
            game.seed, game.choices
        );
    }
}

/// A deliberately broken invariant, that no card is ever in a graveyard, shrinks to a game of
/// a handful of choices that prints as a scenario reaching the same position. It panics, as
/// the engine's debug assertions do, rather than returning a violation.
#[test]
fn a_broken_invariant_shrinks_to_a_scenario() {
    #[allow(clippy::unnecessary_wraps)] // A `Check`, failing by panicking.
    fn graveyards_stay_empty(_: &GameTable, s: &State) -> Result<(), String> {
        for p in Seat::BOTH {
            assert!(
                s.zone(ZoneKey::Of(p, Zone::Graveyard)).is_empty(),
                "a card is in P{}'s graveyard",
                p.get()
            );
        }
        Ok(())
    }
    let (game, why) = shrink(4, graveyards_stay_empty).expect("a card reaches a graveyard");
    let (text, scenario) = game.scenario(graveyards_stay_empty, &why);
    println!("seed {}, choices {:?}\n{text}", game.seed, game.choices);
    assert!(game.choices.len() <= 12, "{:?}\n{text}", game.choices);
    let scenario = scenario.expect("the game reached a priority decision");
    // The game's decisions from its first priority on, as the scenario must meet them.
    let mut decisions = Vec::new();
    let _ = game.play(graveyards_stay_empty, |t, s, d, _| {
        decisions.push(seen(t, s, d));
    });
    let decisions = &decisions[decisions.len() - scenario.actions.len()..];
    let mut live = Live::new(&CARDS, &scenario).unwrap();
    for (&action, expected) in scenario.actions.iter().zip(decisions) {
        let Ok(Step::Decide(d)) = live.position(action) else {
            panic!("no decision for {action:?}\n{text}");
        };
        assert_eq!(&seen(live.table(), live.state(), &d), expected, "{text}");
        live.act(action).unwrap_or_else(|f| panic!("{f}\n{text}"));
    }
    live.snapshot().unwrap();
    assert!(
        catch_unwind(|| graveyards_stay_empty(live.table(), live.state())).is_err(),
        "the scenario stops short of the failure:\n{text}"
    );
}

/// A decision by names alone, which the scenario's own game shares: who decides what, when,
/// among which candidates as actions, with both hands.
fn seen(t: &GameTable, s: &State, d: &Decision) -> String {
    let names = |mut names: Vec<String>| {
        names.sort_unstable();
        names
    };
    let candidates = names(
        (0..d.len() as u16)
            .map(|c| script(t, s, d, Choice(c)).map_or_else(|| "?".to_owned(), |a| source(&a)))
            .collect(),
    );
    let hands = Seat::BOTH.map(|p| {
        names(
            s.in_candidate_order(s.zone(ZoneKey::Of(p, Zone::Hand)))
                .map(|slot| s.name(t, slot).to_owned())
                .collect(),
        )
    });
    let turn = s.turn();
    format!(
        "turn {} {:?}: {candidates:?}; hands {hands:?}",
        turn.number, turn.step
    )
}
