//! A turn is numbered the way players count them: each player's own turns, so the starting
//! player's first turn and the other player's first turn are both "turn 1".

use beryl_cards::CARDS;
use beryl_cards::names::{ISLAND, THUNDERTRAP_TRAINER};
use beryl_session::protocol::Side;
use beryl_session::{CandidateIndex, Game, Seat, SessionId, StartGame, Status, StopsSetting};

#[test]
fn each_players_first_turn_is_turn_one() {
    let deck = vec![
        (24, ISLAND.to_owned()),
        (16, THUNDERTRAP_TRAINER.to_owned()),
    ];
    let start = StartGame {
        seed: 7,
        decks: [deck.clone(), deck],
        human: Seat::P0,
        opponent: "random".to_owned(),
        stops: StopsSetting::default(),
    };
    let mut game = Game::new(&CARDS, &start, SessionId(1)).unwrap();
    let (mut last, mut taken) = (0, [0u16; 2]);
    let mut seen_turns = 0;
    while let Some(view) = game.next_view() {
        if view.turn.number > 6 {
            break;
        }
        if view.turn.number != last {
            last = view.turn.number;
            taken[usize::from(view.turn.active == Side::Opponent)] += 1;
            seen_turns += 1;
        }
        if view.turn.number > 0 {
            let side = usize::from(view.turn.active == Side::Opponent);
            assert_eq!(
                view.turn.own_number, taken[side],
                "turn {}",
                view.turn.number
            );
        }
        if let Status::Deciding { decision } = &view.status {
            let answer = game.answer(decision.id, CandidateIndex(0));
            game.choose(answer.unwrap());
        }
    }
    assert!(seen_turns >= 4, "the game reached its fourth turn");
    assert_eq!(
        taken[0], taken[1],
        "after an even number of turns, each took as many"
    );
}
