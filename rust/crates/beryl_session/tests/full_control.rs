//! Full control, the exact stops: the human gets every priority they receive as a decision,
//! as on Arena, even when passing is all they can do. They see each of the opponent's spells
//! on the stack, in either player's turn, and each step of the opponent's turn.

use beryl_cards::CARDS;
use beryl_cards::names::{GET_LOST, ISLAND, PLAINS, THUNDERTRAP_TRAINER};
use beryl_engine::TurnStep;
use beryl_engine::state::{Pending, StackEntry};
use beryl_session::protocol::{Candidate, Kind};
use beryl_session::{CandidateIndex, Game, Seat, SessionId, StartGame, Status, StopsSetting};

const SESSION: SessionId = SessionId(1);
const HUMAN: Seat = Seat::P0;
const TURNS: u16 = 16;

fn deck(cards: &[(u8, &str)]) -> Vec<(u8, String)> {
    cards
        .iter()
        .map(|&(n, name)| (n, name.to_owned()))
        .collect()
}

/// The human's deck has nothing to cast at instant speed, so passing is all they can do
/// whenever the opponent casts something.
fn new_game(seed: u64, stops: StopsSetting) -> Game {
    let start = StartGame {
        seed,
        decks: [
            deck(&[(24, ISLAND), (16, THUNDERTRAP_TRAINER)]),
            deck(&[(17, PLAINS), (23, GET_LOST)]),
        ],
        human: HUMAN,
        opponent: "random".to_owned(),
        stops,
    };
    Game::new(&CARDS, &start, SESSION).unwrap()
}

/// What the human saw of a game: the opponent's entries that were ever on the stack, those
/// on the stack at one of the human's priorities with whose turn it was, and the steps of
/// the opponent's turns in which the human had priority.
#[derive(Default)]
struct Seen {
    theirs: Vec<StackEntry>,
    answered: Vec<(StackEntry, Seat)>,
    their_steps: Vec<(u16, TurnStep)>,
}

/// Plays into turn [`TURNS`]: the human plays a land, else casts what they can, else passes.
fn play(game: &mut Game) -> Seen {
    play_switching(game, None)
}

/// As [`play`], changing the stops to `switch` mid-game, as the settings toggle does: at the
/// human's first decision in their own turn from turn 3 on.
fn play_switching(game: &mut Game, mut switch: Option<StopsSetting>) -> Seen {
    let mut seen = Seen::default();
    while let Some(mut view) = game.next_view() {
        let turn = game.state().turn();
        if matches!(view.status, Status::Deciding { .. })
            && turn.number >= 3
            && turn.active == HUMAN
            && let Some(stops) = switch.take()
        {
            view = game.set_stops(stops).unwrap();
        }
        // A spell being cast is on the stack before its targets are chosen (CR 601.2a).
        let cast = matches!(game.state().pending(), Pending::None);
        for e in game.state().stack().iter().filter(|_| cast) {
            if e.controller != HUMAN && !seen.theirs.contains(e) {
                seen.theirs.push(*e);
            }
        }
        if view.turn.number > TURNS {
            break;
        }
        let Status::Deciding { decision } = &view.status else {
            continue;
        };
        let turn = game.state().turn();
        if decision.kind == Kind::Priority && !decision.halt {
            for e in game.state().stack() {
                if e.controller != HUMAN {
                    seen.answered.push((*e, turn.active));
                }
            }
            if turn.active != HUMAN {
                seen.their_steps.push((turn.number, turn.step));
            }
        }
        let index = decision
            .candidates
            .iter()
            .position(|c| matches!(c.option, Candidate::PlayLand { .. }))
            .or_else(|| {
                decision
                    .candidates
                    .iter()
                    .position(|c| matches!(c.option, Candidate::Cast { .. }))
            })
            .unwrap_or(0);
        let answer = game.answer(decision.id, CandidateIndex(index as u16));
        game.choose(answer.unwrap());
    }
    assert_eq!(switch, None, "the stops never changed");
    seen
}

#[test]
fn full_control_gives_priority_over_every_opponent_spell_in_either_turn() {
    let mut in_turn = [0, 0];
    for seed in 0..4 {
        let seen = play(&mut new_game(seed, StopsSetting::EXACT));
        for e in &seen.theirs {
            assert!(
                seen.answered.iter().any(|(a, _)| a == e),
                "seed {seed}: no priority over the opponent's {e:?}"
            );
        }
        for (_, active) in &seen.answered {
            in_turn[usize::from(*active == HUMAN)] += 1;
        }
    }
    assert!(in_turn[0] > 0, "the opponent cast nothing in their turn");
    assert!(
        in_turn[1] > 0,
        "the opponent cast nothing in the human's turn"
    );
}

#[test]
fn full_control_stops_in_every_step_of_the_opponents_turn() {
    assert_every_step(&play(&mut new_game(0, StopsSetting::EXACT)));
}

#[test]
fn switching_to_full_control_mid_game_stops_in_every_step() {
    let mut game = new_game(0, StopsSetting::SMART);
    assert_every_step(&play_switching(&mut game, Some(StopsSetting::EXACT)));
}

/// The human had priority in each step of each of the opponent's turns that has one.
fn assert_every_step(seen: &Seen) {
    let mut turns: Vec<u16> = seen.their_steps.iter().map(|&(n, _)| n).collect();
    turns.dedup();
    assert!(turns.len() > 4, "only {} opponent turns", turns.len());
    for n in turns {
        for step in [
            TurnStep::Upkeep,
            TurnStep::PrecombatMain,
            TurnStep::BeginningOfCombat,
            TurnStep::PostcombatMain,
            TurnStep::End,
        ] {
            assert!(
                seen.their_steps.contains(&(n, step)),
                "no priority in the opponent's {step:?} of turn {n}"
            );
        }
    }
}

#[test]
fn smart_stops_pass_where_the_human_can_only_pass() {
    let seen = play(&mut new_game(0, StopsSetting::SMART));
    assert!(
        seen.their_steps
            .iter()
            .all(|&(_, step)| step != TurnStep::Upkeep),
        "smart stops stopped in the opponent's upkeep"
    );
}
