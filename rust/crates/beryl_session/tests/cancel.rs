//! Taking back a cast before its costs are paid: the game returns to the human's priority as
//! it was, and the replay never records the cast.

use beryl_cards::CARDS;
use beryl_cards::decks::WU_CONTROL;
use beryl_engine::Seat;
use beryl_session::protocol::{Candidate, Kind};
use beryl_session::replay::Entry;
use beryl_session::{
    CandidateIndex, Game, PlayerView, SessionError, SessionId, StartGame, Status, StopsSetting,
};

fn owned(deck: &[(u8, &str)]) -> Vec<(u8, String)> {
    deck.iter().map(|&(n, name)| (n, name.to_owned())).collect()
}

const SESSION: SessionId = SessionId(1);

fn new_game() -> Game {
    let start = StartGame {
        seed: 1,
        decks: [owned(WU_CONTROL), owned(WU_CONTROL)],
        human: Seat::P0,
        opponent: "random".to_owned(),
        stops: StopsSetting::Smart,
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

fn deciding(view: &PlayerView) -> Option<&beryl_session::protocol::DecisionView> {
    match &view.status {
        Status::Deciding { decision } => Some(decision),
        _ => None,
    }
}

/// Answers `index` and plays on to the human's next decision.
fn answer(game: &mut Game, view: &PlayerView, index: usize) -> PlayerView {
    let id = deciding(view).unwrap().id;
    game.choose(game.answer(id, CandidateIndex(index as u8)).unwrap());
    to_human(game)
}

/// Plays the game with the human passing or playing a land, until they hold priority with a
/// spell whose cast asks for a target: the view there, and that cast's index.
fn to_a_targeted_cast(game: &mut Game) -> (PlayerView, usize) {
    let mut view = to_human(game);
    for _ in 0..400 {
        let d = deciding(&view).expect("the game goes on");
        if d.kind == Kind::Priority && !d.halt {
            for (i, c) in d.candidates.iter().enumerate() {
                if !matches!(c.option, Candidate::Cast { .. }) {
                    continue;
                }
                // Look ahead in a copy of the game: does the cast ask a target?
                let mut ahead = Game::from_replay(&CARDS, game.replay(), SESSION).unwrap().0;
                let next = answer(&mut ahead, &view, i);
                if deciding(&next).is_some_and(|n| n.kind == Kind::Target) {
                    return (view, i);
                }
            }
        }
        let land = d
            .candidates
            .iter()
            .position(|c| matches!(c.option, Candidate::PlayLand { .. }));
        // Pass, or play a land; the first candidate of any other decision.
        let pass = d
            .candidates
            .iter()
            .position(|c| matches!(c.option, Candidate::Pass));
        view = answer(game, &view, land.or(pass).unwrap_or(0));
    }
    panic!("no targeted cast in 400 decisions");
}

#[test]
fn a_cast_taken_back_at_its_target_returns_to_priority_as_it_was() {
    let mut game = new_game();
    let (before, cast) = to_a_targeted_cast(&mut game);
    assert!(
        !deciding(&before).unwrap().cancellable,
        "priority itself is not"
    );
    let state = *game.state();
    let log = game.replay().log.clone();

    let targeting = answer(&mut game, &before, cast);
    let asked = deciding(&targeting).unwrap();
    assert_eq!(asked.kind, Kind::Target);
    assert!(asked.cancellable, "an unpaid cast can be taken back");
    assert!(game.open().unwrap().cancel(asked.id).is_ok());

    let again = game.cancel().unwrap();
    let d = deciding(&again).unwrap();
    assert_eq!(d.kind, Kind::Priority);
    assert!(!d.cancellable);
    assert_ne!(d.id, deciding(&before).unwrap().id, "a new decision");
    assert!(again.events.is_empty(), "nothing happened");
    assert_eq!(*game.state(), state, "the game is as it was");
    assert_eq!(game.replay().log, log, "the replay forgets the cast");
    assert_eq!(again.objects, before.objects);
    assert_eq!(again.stack, before.stack);
    assert_eq!(game.cancel(), Err(SessionError::NotCancellable));
    // The old decision's answers are stale.
    assert!(game.answer(asked.id, CandidateIndex(0)).is_err());

    // Cast again, and switch to full control while choosing its target: taking the cast back
    // keeps the new stops, recorded at the priority.
    let targeting = answer(&mut game, &again, cast);
    assert!(deciding(&targeting).unwrap().cancellable);
    game.set_stops(StopsSetting::Exact).unwrap();
    let again = game.cancel().unwrap();
    assert_eq!(again.stops, StopsSetting::Exact);
    assert_eq!(game.replay().log[..log.len()], log[..]);
    assert_eq!(
        game.replay().log[log.len()..],
        [Entry::Stops(StopsSetting::Exact)]
    );

    // Cast again and see it through: once its costs are paid, it stays cast.
    let mut view = answer(&mut game, &again, cast);
    while let Some(d) = deciding(&view)
        && d.cancellable
    {
        view = answer(&mut game, &view, 0);
    }
    assert!(!view.stack.is_empty(), "the spell was cast");
    assert_eq!(game.cancel(), Err(SessionError::NotCancellable));
    // The replay reproduces the game without the cast taken back, but for the count of
    // decisions in the ids, which counted the one taken back.
    let (_, mut replayed) = Game::from_replay(&CARDS, game.replay(), SESSION).unwrap();
    if let (Status::Deciding { decision: r }, Status::Deciding { decision: v }) =
        (&mut replayed.status, &view.status)
    {
        assert!(r.id.seq < v.id.seq);
        r.id = v.id;
    }
    assert_eq!(replayed, view);
}
