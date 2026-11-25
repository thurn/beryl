//! Saves of games with a turn one player controls for another (Mindslaver, CR 724) and with
//! hidden picks (Show and Tell, CR 101.4a). Each replays exactly to the same game with its
//! seed derived another way, its candidates listed in other orders and its cards in other
//! slots, from the end of the game and from a save cut off in the middle of a controlled turn
//! and of a hidden pick.

use beryl_cards::CARDS;
use beryl_session::perturb::{self, Perturbation};
use beryl_session::protocol::ObjectView;
use beryl_session::replay::Entry;
use beryl_session::save::Described;
use beryl_session::{
    CandidateIndex, Game, PlayerView, ReplayFile, Seat, SessionId, StartGame, Status, StopsSetting,
};

const SESSION: SessionId = SessionId(4);

/// Islands, Mindslaver, Show and Tell and creatures for it to put onto the battlefield.
fn deck() -> Vec<(u8, String)> {
    vec![
        (24, "Island".to_owned()),
        (2, "Mana Crypt".to_owned()),
        (5, "Mindslaver".to_owned()),
        (5, "Show and Tell".to_owned()),
        (4, "Grizzly Bears".to_owned()),
    ]
}

fn to_human(game: &mut Game) -> PlayerView {
    let mut last = None;
    while let Some(view) = game.next_view() {
        last = Some(view);
    }
    last.expect("the game plays on from where it is")
}

/// Where a game of [`played`] stops.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Until {
    /// After this many of the human's answers.
    Answers(usize),
    /// At the human's first decision in a turn one player controls for the other.
    Controlled,
    /// At the human's first decision while a hidden pick is marked.
    HiddenPick,
}

fn hidden_pick(view: &PlayerView) -> bool {
    view.objects.iter().any(|o: &ObjectView| o.hidden_pick)
}

/// A game in which the human plays an Island, casts Mana Crypt, Show and Tell, Mindslaver and
/// Grizzly Bears whenever they can, activates Mindslaver at the opponent, and puts a creature
/// onto the battlefield with Show and Tell, played until `until`. Also how many controlled
/// turns and hidden picks the human's decisions saw.
fn played(seed: u64, until: Until) -> (Game, PlayerView, (usize, usize)) {
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
    let (mut controlled, mut hidden) = (0, 0);
    for answers in 0.. {
        let Status::Deciding { decision } = &view.status else {
            break;
        };
        let in_control = view.turn.controlled_by.is_some();
        let picking = hidden_pick(&view);
        controlled += usize::from(in_control);
        hidden += usize::from(picking);
        match until {
            Until::Answers(n) if answers >= n => break,
            Until::Controlled if in_control => break,
            Until::HiddenPick if picking => break,
            _ => {}
        }
        let labels: Vec<&str> = (decision.candidates.iter())
            .map(|c| c.label.as_str())
            .collect();
        let wanted = |part: &str| labels.iter().position(|l| l.contains(part));
        let index = wanted("Play Island")
            .or_else(|| wanted("Cast Mana Crypt"))
            .or_else(|| wanted("Mindslaver: "))
            .or_else(|| wanted("Cast Show and Tell"))
            .or_else(|| wanted("Cast Mindslaver"))
            .or_else(|| wanted("Grizzly Bears"))
            .or_else(|| wanted("Opponent"))
            .unwrap_or(0);
        let index = CandidateIndex(u16::try_from(index).unwrap());
        game.choose(game.answer(decision.id, index).unwrap());
        view = to_human(&mut game);
    }
    (game, view, (controlled, hidden))
}

/// What each answer of `file` did.
fn acts(file: &ReplayFile) -> Vec<Option<Described>> {
    (file.log.iter())
        .filter_map(|e| match e {
            Entry::Choice(c) | Entry::Auto(c) => Some(c.what.clone()),
            _ => None,
        })
        .collect()
}

/// The parts of a view a replay must give again.
fn summary(v: &PlayerView) -> impl PartialEq + std::fmt::Debug {
    let mut permanents: Vec<Option<String>> = (v.objects.iter())
        .filter(|o| v.battlefield.contains(&o.slot))
        .map(|o| o.name.clone())
        .collect();
    permanents.sort();
    let (you, them) = (&v.players.you, &v.players.opponent);
    (
        v.turn.clone(),
        (you.life, them.life, you.library.len(), them.library.len()),
        (you.graveyard.len(), them.graveyard.len()),
        permanents,
        v.objects.iter().filter(|o| o.hidden_pick).count(),
    )
}

fn replays_under_each_perturbation(game: &Game, view: &PlayerView) {
    for (name, _, p) in Perturbation::EACH {
        let (again, replayed) =
            perturb::with(p, || Game::replay_exactly(&CARDS, game.replay(), SESSION))
                .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(acts(again.replay()), acts(game.replay()), "{name}");
        assert_eq!(summary(&replayed), summary(view), "{name}");
    }
}

/// A game in which the human controls the opponent's turns.
const CONTROLLING: u64 = 12;
/// A game in which the human makes hidden picks with Show and Tell.
const SHOWING: u64 = 5;

#[test]
fn a_save_with_controlled_turns_replays_under_each_perturbation() {
    let (game, view, (controlled, _)) = played(CONTROLLING, Until::Answers(1500));
    assert!(
        controlled >= 1,
        "no decision in a controlled turn: the test shows too little"
    );
    replays_under_each_perturbation(&game, &view);
}

#[test]
fn a_save_with_hidden_picks_replays_under_each_perturbation() {
    let (game, view, (_, hidden)) = played(SHOWING, Until::Answers(1500));
    assert!(
        hidden >= 1,
        "no decision at a hidden pick: the test shows too little"
    );
    replays_under_each_perturbation(&game, &view);
}

#[test]
fn a_save_cut_off_in_a_controlled_turn_replays_under_each_perturbation() {
    let (game, view, _) = played(CONTROLLING, Until::Controlled);
    assert!(view.turn.controlled_by.is_some(), "no controlled turn");
    replays_under_each_perturbation(&game, &view);
}

#[test]
fn a_save_cut_off_at_a_hidden_pick_replays_under_each_perturbation() {
    let (game, view, _) = played(SHOWING, Until::HiddenPick);
    assert!(hidden_pick(&view), "no hidden pick");
    replays_under_each_perturbation(&game, &view);
}

/// The human's decisions for the player whose turn they control are undo points: an answer
/// to the first of them is taken back to that decision, still in the controlled turn.
#[test]
fn an_undo_in_a_controlled_turn_takes_back_one_answer() {
    let (mut game, view, _) = played(CONTROLLING, Until::Controlled);
    let Status::Deciding { decision } = &view.status else {
        panic!("a decision in the controlled turn");
    };
    game.choose(game.answer(decision.id, CandidateIndex(0)).unwrap());
    let after = to_human(&mut game);
    assert_ne!(
        summary(&after),
        summary(&view),
        "the answer changed nothing"
    );
    let back = game.undo().expect("the answer to take back");
    assert!(back.undone);
    assert_eq!(summary(&back), summary(&view));
}
