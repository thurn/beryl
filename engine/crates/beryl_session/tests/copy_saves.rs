//! Copies in a save. A Phantasmal Image that enters as a copy of a creature is a card under a
//! copy effect: its definition is the creature's, and the card it really is lives in a memo
//! keyed by its slot. A storm copy of Brain Freeze takes a slot of the shared pool. A save
//! of a game with both replays exactly, with no rebuild from a turn's beginning, to the same
//! game with its seed derived another way, its candidates listed in other orders and its
//! cards in other slots.
//!
//! In this game the opponent controls a Grizzly Bears and a Phantasmal Image copying one, both
//! able to attack. Attackers are asked one creature at a time, in slot order, and the save
//! once named both "Grizzly Bears", so with other slot numbers the recorded attack went to the
//! other creature.

use beryl_cards::CARDS;
use beryl_engine::card::Purpose;
use beryl_session::perturb::{self, Perturbation};
use beryl_session::replay::Entry;
use beryl_session::save::Described;
use beryl_session::{
    CandidateIndex, Game, PlayerView, ReplayFile, Seat, SessionId, StartGame, Status, StopsSetting,
};

const SESSION: SessionId = SessionId(3);

/// Lands for both colors, creatures for the Images to copy, and Brain Freeze, whose storm
/// copies it once for each spell cast before it in the turn.
fn deck() -> Vec<(u8, String)> {
    vec![
        (18, "Island".to_owned()),
        (8, "Forest".to_owned()),
        (6, "Grizzly Bears".to_owned()),
        (8, "Phantasmal Image".to_owned()),
        (6, "Brain Freeze".to_owned()),
    ]
}

fn to_human(game: &mut Game) -> PlayerView {
    let mut last = None;
    while let Some(view) = game.next_view() {
        last = Some(view);
    }
    last.expect("the game plays on from where it is")
}

/// What the human did in a game of [`played`] that a copy needs.
#[derive(Default)]
struct Shown {
    /// Images that entered as a copy of a creature.
    images: usize,
    /// Brain Freezes cast after another spell that turn, which storm copies.
    stormed: usize,
}

/// A game in which the human plays a land, casts Grizzly Bears or else Phantasmal Image
/// whenever they can, then Brain Freeze once they have cast another spell that turn or in
/// answer to a spell on the stack, and has
/// each Image copy a creature, played to its `answers`th human decision.
fn played(seed: u64, answers: usize) -> (Game, PlayerView, Shown) {
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
    let mut shown = Shown::default();
    let mut cast_this_turn = (view.turn.number, 0);
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
        if cast_this_turn.0 != view.turn.number {
            cast_this_turn = (view.turn.number, 0);
        }
        let index = if decision.prompt.purpose == Purpose::Choose
            && labels.iter().any(|l| l.contains("Grizzly Bears"))
        {
            shown.images += 1;
            labels
                .iter()
                .position(|l| l.contains("Grizzly Bears"))
                .unwrap_or(0)
        } else if let Some(i) = wanted("Play Island").or_else(|| wanted("Play Forest")) {
            i
        } else if let Some(i) =
            wanted("Cast Brain Freeze").filter(|_| cast_this_turn.1 > 0 || !view.stack.is_empty())
        {
            shown.stormed += 1;
            cast_this_turn.1 += 1;
            i
        } else if let Some(i) =
            wanted("Cast Grizzly Bears").or_else(|| wanted("Cast Phantasmal Image"))
        {
            cast_this_turn.1 += 1;
            i
        } else {
            0
        };
        let index = CandidateIndex(u16::try_from(index).unwrap());
        game.choose(game.answer(decision.id, index).unwrap());
        view = to_human(&mut game);
    }
    (game, view, shown)
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
fn a_save_with_copies_replays_the_same_under_each_perturbation() {
    let (game, view, shown) = played(2, 400);
    assert!(
        shown.images >= 2 && shown.stormed >= 1,
        "{} Images copied a creature and {} Brain Freezes were stormed: the test shows too little",
        shown.images,
        shown.stormed,
    );
    let zones = |v: &PlayerView| {
        let (you, them) = (&v.players.you, &v.players.opponent);
        let sizes = |p: &beryl_session::protocol::PlayerState| (p.library.len(), p.graveyard.len());
        (sizes(you), sizes(them))
    };
    // The permanents by name, which shows what each Image copied.
    let permanents = |v: &PlayerView| {
        let mut names: Vec<(Option<String>, bool)> = (v.objects.iter())
            .filter(|o| v.battlefield.contains(&o.slot))
            .map(|o| (o.name.clone(), o.token))
            .collect();
        names.sort();
        names
    };
    for (name, _, p) in Perturbation::EACH {
        let (again, replayed) =
            perturb::with(p, || Game::replay_exactly(&CARDS, game.replay(), SESSION))
                .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(acts(again.replay()), acts(game.replay()), "{name}");
        assert_eq!(replayed.turn, view.turn, "{name}");
        assert_eq!(zones(&replayed), zones(&view), "{name}");
        assert_eq!(permanents(&replayed), permanents(&view), "{name}");
        assert_eq!(
            (replayed.players.you.life, replayed.players.opponent.life),
            (view.players.you.life, view.players.opponent.life),
            "{name}"
        );
    }
}
