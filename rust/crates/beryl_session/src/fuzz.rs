//! Session games that shrink: whole games
//! played through a [`Game`], each update drawn for both seats with its script, in a build with
//! debug assertions on, so that every script is held to the replay check and the rule
//! invariants where it is built (`script/build.rs`). `BERYL_FUZZ_GAMES=<n>` plays more games,
//! as `just fuzz` does.
//!
//! A game is a seed, which deals two fuzz decks, picks the human's seat and stops and draws the
//! game's randomness, and the choices taken at its decisions in the order the engine asks them
//! of either seat, each an index taken modulo the decision's candidate count: the human answers
//! through the session as a client does, the other seat is an agent reading the same list. It
//! plays until its choices run out or it ends. proptest generates the choices and, when a game
//! panics, shrinks them to the fewest that still do, as `beryl_search`'s `tests/shrink.rs` does
//! for engine games; the test prints the seed, the choices and the panic, which names the
//! script that broke a check and holds its text.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, PoisonError};

use beryl_cards::CARDS;
use beryl_cards::decks::fuzz_deck;
use beryl_engine::{Choice, Decision, GameTable, Layout, Observation, Profile, Seat, State};
use beryl_search::Agent;
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::test_runner::{Config, RngAlgorithm, TestError, TestRng, TestRunner};

use crate::script::{Act, PLANTED, Script};
use crate::{CandidateIndex, Game, Prepared, SessionId, Status, StopsSetting};

/// The most choices a game is generated with: past most random games' ends.
const MOST_CHOICES: usize = 600;

/// The most choices of a game in the planted fault's test: a card is tapped long before.
const FEW_CHOICES: usize = 40;

/// A fuzz game: its seed and the choice at each decision, by index modulo the candidates.
#[derive(Clone, Debug)]
struct Fuzz {
    seed: u64,
    choices: Vec<u16>,
}

/// The choices of a game as both seats take them, shared by the agent in the other seat and
/// the loop answering the human: the choices to take, and the indices taken so far.
#[derive(Clone, Default)]
struct Picks(Arc<Mutex<(Vec<u16>, Vec<u16>)>>);

impl Picks {
    /// The next choice at a decision of `n` candidates, as an index, or None once the
    /// choices have run out.
    fn take(&self, n: usize) -> Option<u16> {
        let mut picks = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let (choices, taken) = &mut *picks;
        let &choice = choices.get(taken.len())?;
        let index = u16::try_from(usize::from(choice) % n).expect("a candidate index");
        taken.push(index);
        Some(index)
    }

    fn exhausted(&self) -> bool {
        let picks = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        picks.1.len() >= picks.0.len()
    }

    fn taken(&self) -> Vec<u16> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .1
            .clone()
    }
}

impl<L: Layout> Agent<L> for Picks {
    fn choose(&mut self, _: Observation<'_, L>, d: &Decision<L>) -> Choice {
        // The loop stops the game before the agent's decision once the choices have run out.
        Choice(self.take(d.len()).expect("a choice left for the agent"))
    }
}

impl Fuzz {
    fn human(&self) -> Seat {
        if self.seed.is_multiple_of(2) {
            Seat::P0
        } else {
            Seat::P1
        }
    }

    fn stops(&self) -> StopsSetting {
        if (self.seed / 2).is_multiple_of(2) {
            StopsSetting::SMART
        } else {
            StopsSetting::EXACT
        }
    }

    /// The game's session, its other seat playing `picks`.
    fn session(&self, picks: &Picks) -> Game {
        let decks = [
            fuzz_deck(self.seed),
            fuzz_deck(self.seed.wrapping_add(1 << 32)),
        ];
        let table = GameTable::new(&CARDS, [decks[0].as_slice(), decks[1].as_slice()]).unwrap();
        let state = State::new(&table, self.seed, Profile::SMART).unwrap();
        let stand_in = crate::ai::new_agent("random", &table, self.seed).unwrap();
        Game::prepared(
            Prepared {
                table,
                state,
                human: self.human(),
                stops: self.stops(),
                opponent: Box::new(picks.clone()),
                stand_in,
                hand_over: None,
                scripted: false,
            },
            SessionId(1),
        )
    }

    /// Plays the game, drawing each update for both seats, and returns the indices it took
    /// with why it failed, if it did: a panic, or a session that refused an answer.
    fn play(&self) -> (Vec<u16>, Result<(), String>) {
        let picks = Picks(Arc::new(Mutex::new((self.choices.clone(), Vec::new()))));
        let played = catch_unwind(AssertUnwindSafe(|| {
            let mut game = self.session(&picks);
            let stop = game.stop_flag();
            loop {
                // The agent's next decision is not made: the game shows that it stopped.
                if picks.exhausted() {
                    stop.store(true, Ordering::Relaxed);
                }
                let Some(view) = game.next_view() else {
                    return Ok(());
                };
                game.opponent_view();
                let Status::Deciding { decision } = &view.status else {
                    continue;
                };
                let Some(index) = picks.take(decision.candidates.len()) else {
                    return Ok(());
                };
                let answer = game
                    .answer(decision.id, CandidateIndex(index))
                    .map_err(|e| format!("the session refused an answer: {e:?}"))?;
                game.choose(answer);
            }
        }))
        .unwrap_or_else(|panic| {
            Err(panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|&m| m.to_owned()))
                .map_or_else(|| "a panic".to_owned(), |m| format!("a panic: {m}")))
        });
        let taken = picks.taken();
        let n = taken.len();
        (
            taken,
            played.map_err(|why| format!("after {n} choices: {why}")),
        )
    }
}

/// Runs `cases` fuzz games of at most `most` choices, deterministically, and returns the game
/// proptest shrank a failure to, with each choice as the index it took, and why it fails.
fn shrink(cases: u32, most: usize) -> Option<(Fuzz, String)> {
    let config = Config {
        cases,
        failure_persistence: None,
        max_shrink_iters: 100_000,
        ..Config::default()
    };
    let mut runner =
        TestRunner::new_with_rng(config, TestRng::deterministic_rng(RngAlgorithm::ChaCha));
    let games = (0..u64::from(u32::MAX), vec(any::<u16>(), 0..most))
        .prop_map(|(seed, choices)| Fuzz { seed, choices });
    match runner.run(&games, |game| game.play().1.map_err(TestCaseError::fail)) {
        Ok(()) => None,
        Err(TestError::Fail(why, game)) => {
            let (choices, _) = game.play();
            Some((
                Fuzz {
                    seed: game.seed,
                    choices,
                },
                why.to_string(),
            ))
        }
        Err(TestError::Abort(why)) => panic!("fuzzing stopped: {why}"),
    }
}

/// How many games to play: `BERYL_FUZZ_GAMES` when set, for a long run on demand, else
/// `default`.
fn games(default: u32) -> u32 {
    std::env::var("BERYL_FUZZ_GAMES").map_or(default, |n| n.parse().expect("a game count"))
}

/// The failure as the test prints it.
fn report(game: &Fuzz, why: &str) -> String {
    let stops = if game.stops() == StopsSetting::SMART {
        "smart"
    } else {
        "exact"
    };
    format!(
        "seed {} (human P{}, {stops} stops), choices {:?}: {why}",
        game.seed,
        game.human().get(),
        game.choices
    )
}

/// Every script of every update of every fuzz game, for both seats, replays to its view and
/// keeps the rule invariants. Slow CI alone runs it (`verdicts.SLOW_ONLY`); `just fuzz` runs it
/// at length, and the nightly with it.
#[test]
fn session_games_keep_the_script_checks() {
    if let Some((game, why)) = shrink(games(3), MOST_CHOICES) {
        panic!("{}", report(&game, &why));
    }
}

/// A builder that drops every step tapping a card shrinks to a game of a handful of choices,
/// whose report names the seed, the choices and the script that failed the replay check.
#[test]
fn a_planted_builder_fault_shrinks_to_a_few_choices() {
    fn untapped(script: &mut Script) {
        for beat in &mut script.beats {
            beat.retain(|step| !matches!(step.act, Act::Tap { .. }));
        }
        script.beats.retain(|beat| !beat.is_empty());
    }
    /// Takes the fault out again however the test ends.
    struct Planted;
    impl Drop for Planted {
        fn drop(&mut self) {
            PLANTED.set(None);
        }
    }
    PLANTED.set(Some(untapped));
    let _planted = Planted;
    let (game, why) = shrink(8, FEW_CHOICES).expect("a card is tapped in some game");
    let report = report(&game, &why);
    println!("{report}");
    assert!(game.choices.len() <= 12, "{report}");
    assert!(
        why.contains("a script does not replay to its view") && why.contains("in the script"),
        "{report}"
    );
    // The shrunk game replays to the same failure, and one choice fewer plays clean.
    assert_eq!(game.play().1.as_ref().err(), Some(&why), "{report}");
    let mut shorter = game.clone();
    if shorter.choices.pop().is_some() {
        assert_eq!(shorter.play().1, Ok(()), "{report}");
    }
}
