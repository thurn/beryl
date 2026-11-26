use beryl_engine::Seat;
use beryl_session::ReplayFile;

use crate::common::TEST;
use beryl_cards::CARDS;
use beryl_cards::decks::WU_CONTROL;
use beryl_session::protocol::{Candidate, DecisionView, Kind};
use beryl_session::replay::Entry;
use beryl_session::{CandidateIndex, Game, PlayerView, SessionId, StartGame, Status, StopsSetting};

use beryl_cli::review::*;

/// The view at the human's next decision or the end.
fn to_human(game: &mut Game) -> PlayerView {
    let mut last = None;
    while let Some(view) = game.next_view() {
        last = Some(view);
    }
    last.expect("the game plays on")
}

fn deciding(view: &PlayerView) -> Option<&DecisionView> {
    match &view.status {
        Status::Deciding { decision } => Some(decision),
        _ => None,
    }
}

fn find(d: &DecisionView, f: fn(&Candidate) -> bool) -> Option<usize> {
    d.candidates.iter().position(|c| f(&c.option))
}

/// A game against greedy in which the human plays a land when they can and otherwise
/// passes, for `answers` answers. With `undo`, once they can from the fifth turn on, they
/// take back their answers until they stand in the turn before, across the agent's, then
/// answer as before, which plays the same game again.
fn play(answers: usize, undo: bool) -> (ReplayFile, bool) {
    let start = StartGame {
        format: beryl_session::Format::CONSTRUCTED,
        seed: 3,
        decks: [WU_CONTROL, WU_CONTROL]
            .map(|deck| deck.iter().map(|&(n, name)| (n, name.to_owned())).collect()),
        human: Seat::P0,
        opponent: "greedy".to_owned(),
        stops: StopsSetting::SMART,
        sideboards: Default::default(),
    };
    let mut game = Game::new(&CARDS, &start, SessionId(1)).unwrap();
    let mut view = to_human(&mut game);
    let (mut undone, mut answered) = (false, 0);
    while answered < answers {
        let Some(d) = deciding(&view) else { break };
        if undo && !undone && d.undo > 0 && view.turn.number >= 5 {
            let from = view.turn.number;
            while view.turn.number == from {
                view = game.undo().unwrap();
                answered -= 1;
            }
            undone = true;
            continue;
        }
        let pick = if d.kind == Kind::Priority {
            find(d, |c| matches!(c, Candidate::PlayLand { .. }))
                .or_else(|| find(d, |c| matches!(c, Candidate::Pass)))
                .unwrap_or(0)
        } else {
            0
        };
        let index = CandidateIndex(u16::try_from(pick).unwrap());
        game.choose(game.answer(d.id, index).unwrap());
        view = to_human(&mut game);
        answered += 1;
    }
    (game.replay().clone(), undone)
}

fn reviewed(file: &ReplayFile, name: &str) -> String {
    let path =
        std::env::temp_dir().join(format!("beryl-review-{}-{name}.json", std::process::id()));
    file.save(&path).unwrap();
    let text = review(&TEST, &path, Some("greedy"), None);
    std::fs::remove_file(&path).unwrap();
    text.unwrap()
}

/// A replay with an undo reviews as the game went: the game up to the undo, then again from
/// the answer taken back, as a game played straight through shows it.
#[test]
fn review_goes_back_with_an_undo() {
    let (with_undo, undone) = play(40, true);
    assert!(undone && with_undo.log.contains(&Entry::Undo));
    let (straight, _) = play(40, false);
    let with_undo = reviewed(&with_undo, "undo");
    let straight = reviewed(&straight, "straight");
    let (before, _) = with_undo
        .split_once("(the human takes back an answer)")
        .expect("the review notes the undo");
    let (_, after) = with_undo
        .rsplit_once("(the human takes back an answer)")
        .unwrap();
    // The review heads the step gone back to again; from the next turn it reads as the
    // game played straight through.
    let after = &after[after.find("\n== Turn 4,").expect("back in turn 3")..];
    assert!(straight.starts_with(before), "before the undo:\n{before}");
    assert!(straight.ends_with(after), "after the undo:\n{after}");
    assert!(
        before.len() + after.len() > straight.len(),
        "the undo took back nothing"
    );
}
