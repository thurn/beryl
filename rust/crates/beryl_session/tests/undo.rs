//! Going back to the start of the human's turn: the game is exactly as it stood there, the
//! replay goes back the same way, the same answers play the same game again, and the view says
//! when the human has seen a card since that they did not know there.

use beryl_cards::CARDS;
use beryl_cards::decks::WU_CONTROL;
use beryl_engine::{Seat, State};
use beryl_session::protocol::{Candidate, DecisionView, Kind, Side, UndoView};
use beryl_session::replay::Entry;
use beryl_session::{
    CandidateIndex, Game, PlayerView, SessionError, SessionId, StartGame, Status, StopsSetting,
};

fn owned(deck: &[(u8, &str)]) -> Vec<(u8, String)> {
    deck.iter().map(|&(n, name)| (n, name.to_owned())).collect()
}

const SESSION: SessionId = SessionId(1);

fn new_game(seed: u64) -> Game {
    let start = StartGame {
        seed,
        decks: [owned(WU_CONTROL), owned(WU_CONTROL)],
        human: Seat::P0,
        // Greedy has no random stream, so the same answers play the same game again.
        opponent: "greedy".to_owned(),
        stops: StopsSetting::SMART,
    };
    Game::new(&CARDS, &start, SESSION).unwrap()
}

/// Plays on to the human's next decision or the end, and returns the view there.
fn to_human(game: &mut Game) -> PlayerView {
    let mut last = None;
    while let Some(view) = game.next_view() {
        last = Some(view);
    }
    last.expect("the game plays on from where it is")
}

fn deciding(view: &PlayerView) -> Option<&DecisionView> {
    match &view.status {
        Status::Deciding { decision } => Some(decision),
        _ => None,
    }
}

/// Answers `index` and plays on to the human's next decision.
fn answer(game: &mut Game, view: &PlayerView, index: usize) -> PlayerView {
    let id = deciding(view).unwrap().id;
    game.choose(
        game.answer(id, CandidateIndex(u16::try_from(index).unwrap()))
            .unwrap(),
    );
    to_human(game)
}

/// A human who plays a land when they can, casts every third spell they could, and otherwise
/// passes or takes the first candidate.
fn policy(d: &DecisionView, n: usize) -> usize {
    let find = |f: fn(&Candidate) -> bool| d.candidates.iter().position(|c| f(&c.option));
    if d.kind != Kind::Priority {
        return 0;
    }
    find(|c| matches!(c, Candidate::PlayLand { .. }))
        .or_else(|| {
            n.is_multiple_of(3)
                .then(|| find(|c| matches!(c, Candidate::Cast { .. })))
                .flatten()
        })
        .or_else(|| find(|c| matches!(c, Candidate::Pass)))
        .unwrap_or(0)
}

/// The start of a human turn, as the test saw it.
struct Start {
    state: State,
    view: PlayerView,
    /// The human's answers since.
    answers: Vec<usize>,
}

fn without_id(mut view: PlayerView) -> PlayerView {
    if let Status::Deciding { decision } = &mut view.status {
        decision.id.seq = 0;
    }
    view.events.clear();
    view.undone = false;
    view
}

/// Plays `seed`'s game for `decisions` of the human's, and at every `every`th one with
/// something to go back to, goes back in a copy of the game and checks it against the start
/// of the turn. Returns how many undos were checked, and how many of them were peeked.
fn play_and_check(seed: u64, decisions: usize, every: usize) -> (usize, usize) {
    let mut game = new_game(seed);
    let mut view = to_human(&mut game);
    let mut start: Option<Start> = None;
    let (mut checked, mut peeked) = (0, 0);
    for n in 0..decisions {
        let Some(d) = deciding(&view) else {
            break;
        };
        let turn = &view.turn;
        if turn.active == Side::You
            && turn.number > 0
            && start
                .as_ref()
                .is_none_or(|s| s.view.turn.number != turn.number)
        {
            assert_eq!(d.undo, None, "nothing to go back to at the turn's start");
            start = Some(Start {
                state: *game.state(),
                view: view.clone(),
                answers: Vec::new(),
            });
        }
        if let (Some(undo), Some(s)) = (d.undo, &start)
            && n % every == 0
        {
            checked += 1;
            peeked += usize::from(undo == UndoView::Peeked);
            check_undo(&game, s, &view);
        }
        let i = policy(d, n);
        if let Some(s) = &mut start {
            s.answers.push(i);
        }
        view = answer(&mut game, &view, i);
    }
    (checked, peeked)
}

/// Goes back in a copy of `game`, at `now`, to `start`, and checks it.
fn check_undo(game: &Game, start: &Start, now: &PlayerView) {
    let (mut probe, resumed) = Game::from_replay(&CARDS, game.replay(), SESSION).unwrap();
    assert_eq!(without_id(resumed.clone()), without_id(now.clone()));
    let id = deciding(now).unwrap().id;
    assert!(probe.open().unwrap().undo(id).is_ok());
    let back = probe.undo().unwrap();
    assert_eq!(*probe.state(), start.state, "the game is as it stood");
    assert!(back.events.is_empty(), "nothing happened");
    assert!(back.undone);
    assert_eq!(without_id(back.clone()), without_id(start.view.clone()));
    assert_eq!(probe.undo(), Err(SessionError::NothingToUndo));
    assert_eq!(probe.replay().log.last(), Some(&Entry::Undo));

    // A replay goes back the same way.
    let (_, replayed) = Game::from_replay(&CARDS, probe.replay(), SESSION).unwrap();
    assert_eq!(without_id(replayed), without_id(back.clone()));

    // The same answers play the same game again.
    let mut view = back;
    for &i in &start.answers {
        view = answer(&mut probe, &view, i);
    }
    assert_eq!(
        probe.state(),
        game.state(),
        "the same answers, the same game"
    );
    assert_eq!(without_id(view), without_id(now.clone()));
}

#[test]
fn going_back_restores_the_turn_start_and_replays() {
    let mut checked = 0;
    let mut peeked = 0;
    for seed in [1, 2] {
        let (c, p) = play_and_check(seed, 160, 2);
        checked += c;
        peeked += p;
    }
    assert!(checked >= 10, "only {checked} undos checked");
    assert!(peeked > 0, "no undo was peeked");
    assert!(peeked < checked, "every undo was peeked");
}

#[test]
fn a_land_from_hand_is_no_peek_but_a_card_from_the_library_is() {
    let mut seen_clean = false;
    let mut seen_peek = false;
    for seed in 4..12 {
        let mut game = new_game(seed);
        let mut view = to_human(&mut game);
        for _ in 0..150 {
            let Some(d) = deciding(&view) else {
                break;
            };
            let library = |v: &PlayerView| v.players.you.library.len();
            let before = library(&view);
            // Casting whatever it can, to draw mid-turn.
            let i = policy(d, 0);
            let land = matches!(d.candidates[i].option, Candidate::PlayLand { .. });
            let first = d.undo.is_none() && view.turn.active == Side::You && view.turn.number > 0;
            let turn = view.turn.number;
            view = answer(&mut game, &view, i);
            let Some(after) = deciding(&view) else {
                break;
            };
            let same_turn = turn > 0 && view.turn.number == turn;
            // The human's first answer of a turn, a land from their own hand, shows them
            // nothing new.
            if first && land && same_turn {
                assert_eq!(after.undo, Some(UndoView::Clean), "a land is no peek");
                seen_clean = true;
            }
            // A card drawn or found mid-turn from the human's own library is one they did not
            // know there.
            if same_turn && view.turn.active == Side::You && library(&view) < before {
                assert_eq!(after.undo, Some(UndoView::Peeked), "a card drawn is a peek");
                seen_peek = true;
            }
        }
    }
    assert!(seen_clean, "no land played first in a turn");
    assert!(seen_peek, "no card drawn mid-turn");
}
