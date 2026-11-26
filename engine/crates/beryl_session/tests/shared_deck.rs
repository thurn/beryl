//! A shared-deck game through the session: started by its deck key, played to the end by a
//! scripted human against the random agent with both seats' views and scripts drawn, and
//! replayed from its file. In a build with debug assertions every script is held to the replay
//! check where it is built (`script/build.rs`), so a step that put a card in a player's own
//! pile, or named the wrong hand for a card taken from the shared library, fails here.

use std::collections::BTreeSet;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use beryl_cards::CARDS;
use beryl_session::protocol::{ObjectView, SHARED_PILE_SIDE, Side, ZoneName};
use beryl_session::script::{Act, Fact};
use beryl_session::{
    CandidateIndex, Game, GameUpdate, PlayerView, ReplayFile, SessionId, Sessions, StartRequest,
    Status,
};

/// A kind of move on the shared piles a script showed, from either seat.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Move {
    /// A card the viewer's opponent drew: from the shared library to their own hand.
    OpponentDraw,
    /// A card put into the shared graveyard from the shared library.
    Mill,
    /// A card put into the shared library from a hand or the graveyard.
    OntoLibrary,
    /// The shared library's order set after a shuffle.
    Shuffle,
}

type Seen = BTreeSet<Move>;

/// Holds `view` to the shape of a shared game's view: its piles are the shared ones, every
/// object in them names [`SHARED_PILE_SIDE`], and no object is in two piles.
fn check_view(view: &PlayerView) {
    let shared = view
        .shared
        .as_ref()
        .expect("a shared game's view has shared piles");
    for player in [&view.players.you, &view.players.opponent] {
        assert!(player.library.is_empty() && player.graveyard.is_empty());
    }
    let mut seen = BTreeSet::new();
    for &slot in shared.library.iter().chain(&shared.graveyard) {
        assert!(seen.insert(slot), "{slot:?} is in two piles");
    }
    let object = |slot| view.objects.iter().find(|o| o.slot == slot);
    for (pile, zone) in [
        (&shared.library, ZoneName::Library),
        (&shared.graveyard, ZoneName::Graveyard),
    ] {
        for &slot in pile {
            let o = object(slot).unwrap_or_else(|| panic!("{slot:?} has no record"));
            assert_eq!(o.zone, zone);
            assert_eq!(
                (o.owner, o.controller),
                (SHARED_PILE_SIDE, SHARED_PILE_SIDE)
            );
        }
    }
    for o in &view.objects {
        if matches!(o.zone, ZoneName::Library | ZoneName::Graveyard) {
            assert!(seen.contains(&o.slot), "{:?} is in no shared pile", o.slot);
        }
    }
}

/// Notes in `seen` the moves `view`'s script shows.
fn note(view: &PlayerView, seen: &mut Seen) {
    let Some(script) = &view.script else { return };
    for step in script.beats.iter().flatten() {
        let moved = |from: ZoneName, object: &ObjectView| (from, object.zone, object.owner);
        let (from, to, owner) = match &step.act {
            Act::Fly { from, object, .. } | Act::Appear { from, object, .. } => {
                moved(*from, object)
            }
            Act::Vanish {
                from,
                object: Some(object),
                ..
            } => moved(*from, object),
            Act::Set {
                fact: Fact::Library { .. } | Fact::ShuffledLibrary { .. },
            } => {
                seen.insert(Move::Shuffle);
                continue;
            }
            _ => continue,
        };
        match (from, to) {
            (ZoneName::Library, ZoneName::Hand) if owner == Side::Opponent => {
                seen.insert(Move::OpponentDraw);
            }
            (ZoneName::Library, ZoneName::Graveyard) => {
                seen.insert(Move::Mill);
            }
            (ZoneName::Hand | ZoneName::Graveyard, ZoneName::Library) => {
                seen.insert(Move::OntoLibrary);
            }
            _ => {}
        }
    }
}

/// The next update a session emits; a session that stops emitting fails the test rather than
/// hanging it.
fn next(updates: &Receiver<GameUpdate>) -> GameUpdate {
    updates
        .recv_timeout(Duration::from_secs(120))
        .expect("the session appears hung: it emitted no view within the hang limit")
}

/// Plays the shared game of `seed` to its end, the human answering by a fixed pattern over
/// the candidates, and returns its last view and its replay.
fn play(seed: u32, moves: &mut Seen) -> (PlayerView, ReplayFile) {
    let (sent, updates) = channel();
    let (sent_across, across) = channel();
    let sessions = Sessions::new(&CARDS, None, move |u| sent.send(u).unwrap())
        .with_across(move |u| sent_across.send(u).unwrap());
    let request = StartRequest {
        agent: "random".to_owned(),
        seed: Some(seed),
        fair: false,
        deck: "forgetful-fish".to_owned(),
    };
    let session = sessions.start(&request).unwrap();
    let mut answers = 0u32;
    let last = loop {
        let update = next(&updates);
        assert_eq!(update.session, session);
        let view = update.view;
        check_view(&view);
        note(&view, moves);
        while let Ok(other) = across.try_recv() {
            check_view(&other.view);
            note(&other.view, moves);
        }
        let decision = match &view.status {
            Status::Over { .. } => break view,
            Status::Deciding { decision } => decision,
            _ => continue,
        };
        answers += 1;
        assert!(answers < 2_000, "seed {seed}: the game does not end");
        let n = u32::try_from(decision.candidates.len()).unwrap();
        let index = (decision.id.seq * 7 + 3) % n;
        sessions
            .choose(decision.id, CandidateIndex(u16::try_from(index).unwrap()))
            .unwrap();
    };
    (
        last,
        sessions.current_replay().expect("a dealt game's replay"),
    )
}

/// A game started from a request for the `forgetful-fish` deck is a shared-deck game: it plays
/// to its end with every view showing the shared piles, its scripts show an opponent's draw, a
/// mill, a card put into the library and a shuffle on the shared piles from both seats, and its
/// replay, saved and loaded, reproduces its last view.
#[test]
fn a_requested_shared_game_plays_to_its_end_and_replays() {
    let mut seen = Seen::default();
    for seed in 1..=3 {
        let (last, replay) = play(seed, &mut seen);
        assert!(
            replay.shared_deck,
            "seed {seed}: the replay records the format"
        );
        let dir =
            std::env::temp_dir().join(format!("beryl-shared-deck-{}-{seed}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("replay.json");
        replay.save(&path).unwrap();
        let file = ReplayFile::load(&path, &CARDS).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        let (_, replayed) = Game::from_replay(&CARDS, &file, SessionId(1)).unwrap();
        assert_eq!(
            PlayerView {
                script: None,
                ..replayed
            },
            PlayerView {
                script: None,
                ..last
            },
            "seed {seed}: the replay ends where the game did"
        );
    }
    let all = [
        Move::OpponentDraw,
        Move::Mill,
        Move::OntoLibrary,
        Move::Shuffle,
    ];
    for kind in all {
        assert!(
            seen.contains(&kind),
            "no script showed a move of kind {kind:?}"
        );
    }
}
