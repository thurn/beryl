//! A replacement effect's choice in a save. Dredge is offered as the draw step's draw is about
//! to happen, which pauses the step's turn-based actions on a question
//! (`Pending::StepActions`). A save that answered it replays exactly to the same game with its seed
//! derived another way, its candidates listed in other orders and its cards in other slots.

use beryl_cards::CARDS;
use beryl_engine::card::Purpose;
use beryl_session::perturb::{self, Perturbation};
use beryl_session::replay::Entry;
use beryl_session::save::Described;
use beryl_session::{
    CandidateIndex, Game, PlayerView, ReplayFile, Seat, SessionId, StartGame, Status, StopsSetting,
};

const SESSION: SessionId = SessionId(2);

/// Forests and Life from the Loam, so a Loam cast from hand reaches the graveyard and offers
/// its dredge at its owner's next draw.
fn deck() -> Vec<(u8, String)> {
    vec![
        (24, "Forest".to_owned()),
        (16, "Life from the Loam".to_owned()),
    ]
}

fn to_human(game: &mut Game) -> PlayerView {
    let mut last = None;
    while let Some(view) = game.next_view() {
        last = Some(view);
    }
    last.expect("the game plays on from where it is")
}

/// A game in which the human plays a land and casts Life from the Loam whenever they can, and
/// dredges whenever they are offered it, played to its `answers`th human decision. Returns the
/// game, the view there, and how many dredges were offered as a step's draw was about to
/// happen rather than during a resolution.
fn played(seed: u64, answers: usize) -> (Game, PlayerView, usize) {
    let start = StartGame {
        seed,
        decks: [deck(), deck()],
        human: Seat::P0,
        opponent: "random".to_owned(),
        stops: StopsSetting::EXACT,
        sideboards: Default::default(),
    };
    let mut game = Game::new(&CARDS, &start, SESSION).unwrap();
    let mut view = to_human(&mut game);
    let mut in_a_step = 0;
    for _ in 0..answers {
        let Status::Deciding { decision } = &view.status else {
            break;
        };
        let labels: Vec<&str> = decision
            .candidates
            .iter()
            .map(|c| c.label.as_str())
            .collect();
        let wanted = |prefix: &str| labels.iter().position(|l| l.starts_with(prefix));
        let index = if decision.prompt.purpose == Purpose::Replace {
            if decision.resolving.is_none() {
                in_a_step += 1;
            }
            // Any candidate that names a card dredges it; the other declines.
            labels
                .iter()
                .position(|l| l.contains("Life from the Loam"))
                .unwrap_or(0)
        } else {
            wanted("Play Forest")
                .or_else(|| wanted("Cast Life from the Loam"))
                .unwrap_or(0)
        };
        let index = CandidateIndex(u16::try_from(index).unwrap());
        game.choose(game.answer(decision.id, index).unwrap());
        view = to_human(&mut game);
    }
    (game, view, in_a_step)
}

/// What each answer of `file` did.
fn acts(file: &ReplayFile) -> Vec<Option<Described>> {
    file.log
        .iter()
        .filter_map(|e| match e {
            Entry::Choice(c) | Entry::Auto(c) => Some(c.what.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_save_with_dredge_in_the_draw_step_replays_the_same_under_each_perturbation() {
    let (game, view, in_a_step) = played(5, 150);
    assert!(
        in_a_step >= 2,
        "dredge was offered {in_a_step} times as a step's draw happened: the test shows too little"
    );
    let zones = |v: &PlayerView| {
        let (you, them) = (&v.players.you, &v.players.opponent);
        let sizes = |p: &beryl_session::protocol::PlayerState| (p.library.len(), p.graveyard.len());
        (sizes(you), sizes(them))
    };
    for (name, _, p) in Perturbation::EACH {
        let (again, replayed) =
            perturb::with(p, || Game::replay_exactly(&CARDS, game.replay(), SESSION))
                .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(acts(again.replay()), acts(game.replay()), "{name}");
        assert_eq!(replayed.turn, view.turn, "{name}");
        assert_eq!(zones(&replayed), zones(&view), "{name}");
        assert_eq!(
            (replayed.players.you.life, replayed.players.opponent.life),
            (view.players.you.life, view.players.opponent.life),
            "{name}"
        );
    }
}
