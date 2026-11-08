//! Saves record the board as each turn begins (`beryl_session::checkpoint`): a replay checks
//! itself against each, and where it can no longer go on, the game is rebuilt from the latest
//! turn's beginning before it, unless something off the board would be lost.

use beryl_cards::CARDS;
use beryl_cards::decks::{VINTAGE_WU_CONTROL, WU_CONTROL};
use beryl_engine::Decklist;
use beryl_session::checkpoint::Checkpoint;
use beryl_session::replay::Entry;
use beryl_session::save::Act;
use beryl_session::{
    Game, PlayerView, ReplayFile, Seat, SessionError, SessionId, StartGame, Status, StopsSetting,
};

const SESSION: SessionId = SessionId(1);

fn owned(deck: &Decklist) -> Vec<(u8, String)> {
    deck.iter().map(|&(n, name)| (n, name.to_owned())).collect()
}

/// Plays a game between the two control decks from `seed` to its end, or for 600 of the
/// human's decisions: the random agent in both seats, answering for the human as a stand-in.
fn played(seed: u64) -> (Game, PlayerView) {
    let start = StartGame {
        seed,
        decks: [owned(VINTAGE_WU_CONTROL), owned(WU_CONTROL)],
        human: Seat::P0,
        opponent: "random".to_owned(),
        stops: StopsSetting::EXACT,
    };
    let mut game = Game::new(&CARDS, &start, SESSION).unwrap();
    game.set_stand_in("random").unwrap();
    let mut last = None;
    for _ in 0..600 {
        while let Some(view) = game.next_view() {
            last = Some(view);
        }
        if matches!(last.as_ref().unwrap().status, Status::Over { .. }) {
            break;
        }
        game.answer_by_stand_in();
    }
    while let Some(view) = game.next_view() {
        last = Some(view);
    }
    (game, last.unwrap())
}

/// Each checkpoint of `file`, with where it stands in the log.
fn checkpoints(file: &ReplayFile) -> Vec<(usize, &Checkpoint)> {
    file.log
        .iter()
        .enumerate()
        .filter_map(|(i, e)| match e {
            Entry::Checkpoint(c) => Some((i, &**c)),
            _ => None,
        })
        .collect()
}

/// `file` with the first land played or spell cast in the first turn whose beginning `pick`
/// takes made impossible, naming a card no deck has; and the board as that turn began.
fn broken(
    file: &ReplayFile,
    pick: impl Fn(&Checkpoint) -> bool,
) -> Option<(ReplayFile, Checkpoint)> {
    let marks = checkpoints(file);
    for (k, &(at, board)) in marks.iter().enumerate() {
        if !pick(board) {
            continue;
        }
        let end = marks.get(k + 1).map_or(file.log.len(), |&(i, _)| i);
        let mut broken = file.clone();
        let Some(card) = broken.log[at..end].iter_mut().find_map(|e| match e {
            Entry::Choice(c) => match &mut c.what.as_mut()?.act {
                Act::PlayLand { card } | Act::Cast { card, .. } => Some(card),
                _ => None,
            },
            _ => None,
        }) else {
            continue;
        };
        "No Such Card".clone_into(&mut card.name);
        return Some((broken, board.clone()));
    }
    None
}

/// The board recorded first in `game`'s log.
fn first_board(game: &Game) -> &Checkpoint {
    checkpoints(game.replay())[0].1
}

#[test]
fn each_turn_s_board_is_recorded_and_replays() {
    let (game, view) = played(3);
    let file = game.replay();
    let turns: Vec<u16> = checkpoints(file).iter().map(|(_, c)| c.turn).collect();
    assert!(turns.len() >= 10, "{turns:?}");
    assert_eq!(turns, (1..=turns.len() as u16).collect::<Vec<_>>());
    let json = serde_json::to_string(file).unwrap();
    assert_eq!(&serde_json::from_str::<ReplayFile>(&json).unwrap(), file);
    let (again, replayed) = Game::from_replay(&CARDS, file, SESSION).unwrap();
    assert_eq!(replayed, view);
    assert_eq!(again.replay().log, file.log);
    assert_eq!(again.replay().from, None);
}

#[test]
fn every_board_rebuilds_as_it_was() {
    let (game, _) = played(5);
    let file = game.replay();
    let mut rebuilt = 0;
    for (_, board) in checkpoints(file) {
        if board.lingering.is_some() {
            continue;
        }
        let from = ReplayFile {
            from: Some(Box::new(board.clone())),
            log: Vec::new(),
            ..file.clone()
        };
        let (again, view) = Game::from_replay(&CARDS, &from, SESSION).unwrap();
        assert_eq!(first_board(&again), board, "turn {}", board.turn);
        assert_eq!(view.turn.number, board.turn);
        rebuilt += 1;
    }
    assert!(rebuilt >= 10, "{rebuilt}");
}

#[test]
fn an_impossible_answer_resumes_at_its_turn_s_beginning() {
    let (game, _) = played(3);
    let (file, board) = broken(game.replay(), |c| c.turn >= 4 && c.lingering.is_none())
        .expect("a turn with a land or a spell");
    let (mut again, view) = Game::from_replay(&CARDS, &file, SESSION).unwrap();
    assert_eq!(view.turn.number, board.turn);
    assert_eq!(again.replay().from.as_deref(), Some(&board));
    assert_eq!(first_board(&again), &board);
    // The rebuilt game plays on, and its save replays from where it was rebuilt.
    again.set_stand_in("random").unwrap();
    let mut last = view;
    for _ in 0..40 {
        while let Some(view) = again.next_view() {
            last = view;
        }
        if matches!(last.status, Status::Over { .. }) {
            break;
        }
        again.answer_by_stand_in();
    }
    while let Some(view) = again.next_view() {
        last = view;
    }
    let (resumed, replayed) = Game::from_replay(&CARDS, again.replay(), SESSION).unwrap();
    assert_eq!(replayed, last);
    assert_eq!(resumed.replay().from.as_deref(), Some(&board));
}

#[test]
fn a_board_the_replay_does_not_reach_is_rebuilt_from_the_record() {
    let (game, _) = played(3);
    let mut file = game.replay().clone();
    let (at, board) = checkpoints(&file)
        .into_iter()
        .find(|(_, c)| c.turn >= 4 && c.lingering.is_none())
        .map(|(at, c)| (at, c.clone()))
        .unwrap();
    let mut changed = board;
    changed.players[1].life -= 1;
    file.log[at] = Entry::Checkpoint(Box::new(changed.clone()));
    let (again, view) = Game::from_replay(&CARDS, &file, SESSION).unwrap();
    assert_eq!(view.turn.number, changed.turn);
    assert_eq!(first_board(&again), &changed);
}

#[test]
fn a_lingering_effect_refuses_a_rebuild() {
    let (file, board) = (0..20)
        .find_map(|seed| broken(played(seed).0.replay(), |c| c.lingering.is_some()))
        .expect("a game with something lasting past a turn's beginning");
    let lingering = board.lingering.unwrap();
    let Err(SessionError::BadReplay { reason }) = Game::from_replay(&CARDS, &file, SESSION) else {
        panic!("the broken record replayed");
    };
    assert!(reason.contains("No Such Card"), "{reason}");
    assert!(
        reason.contains(&format!("no rebuild from turn {}'s beginning", board.turn)),
        "{reason}"
    );
    assert!(reason.contains(&lingering), "{reason}");
}
