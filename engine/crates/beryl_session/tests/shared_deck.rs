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
        work: None,
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

struct PickCandidates(std::collections::VecDeque<beryl_session::protocol::Candidate>);

impl<L: beryl_engine::Layout> beryl_search::Agent<L> for PickCandidates {
    fn choose(
        &mut self,
        _: beryl_engine::Observation<'_, L>,
        d: &beryl_engine::Decision<L>,
        _: beryl_search::AgentDecision,
    ) -> beryl_engine::Choice {
        let want = self
            .0
            .pop_front()
            .unwrap_or(beryl_session::protocol::Candidate::Keep);
        let opt = match want {
            beryl_session::protocol::Candidate::FreeMulligan => beryl_engine::Opt::FreeMulligan,
            _ => beryl_engine::Opt::Keep,
        };
        (0..d.len() as u16)
            .map(beryl_engine::Choice)
            .find(|&c| d.candidates.option(c) == Some(opt))
            .unwrap_or(beryl_engine::Choice(0))
    }
}

fn play_free_mulligan_board(
    library: &'static [&'static str],
    p0_free: bool,
) -> (Vec<PlayerView>, Vec<PlayerView>) {
    use beryl_scenario::{P0, Scenario};
    use beryl_session::protocol::Candidate;
    use beryl_session::{Prepared, Seat, StopsSetting};

    let scenario = Scenario::new("free-mulligan-test")
        .pregame(P0)
        .shared_library(library);
    let (table, state) = beryl_scenario::position(&CARDS, &scenario).unwrap();
    let (sent_p0, p0_updates) = channel();
    let (sent_p1, p1_updates) = channel();
    let sessions = Sessions::new(&CARDS, None, move |u| sent_p0.send(u).unwrap())
        .with_across(move |u| sent_p1.send(u).unwrap());
    let p1_choices: std::collections::VecDeque<Candidate> = if p0_free {
        [Candidate::Keep].into()
    } else {
        [Candidate::FreeMulligan, Candidate::Keep].into()
    };
    let random = beryl_search::agent("random").unwrap();
    let session = sessions
        .start_prepared(Prepared {
            opponent: Box::new(PickCandidates(p1_choices)),
            stand_in: (random.new)(&table, 2, beryl_search::Budget::DEFAULT),
            scripted: false,
            hand_over: Some(Box::new(|| true)),
            table,
            state,
            human: Seat::P0,
            stops: StopsSetting::SMART,
        })
        .unwrap();

    let first = next(&p0_updates);
    assert_eq!(first.session, session);
    let Status::Deciding { decision } = &first.view.status else {
        panic!("expected P0 mulligan decision");
    };
    let labels: Vec<&str> = decision
        .prompt
        .buttons
        .iter()
        .map(|b| b.label.as_str())
        .collect();
    let wanted = if p0_free {
        assert_eq!(labels, ["Keep 7", "Mulligan", "Free Mulligan"]);
        Candidate::FreeMulligan
    } else {
        assert_eq!(labels, ["Keep 7", "Mulligan"]);
        Candidate::Keep
    };
    let idx = decision
        .candidates
        .iter()
        .position(|c| c.option == wanted)
        .unwrap();
    sessions
        .choose(decision.id, CandidateIndex(u16::try_from(idx).unwrap()))
        .unwrap();

    let mut p0_all = vec![first.view];
    let mut p1_all = Vec::new();
    for _ in 0..8 {
        if let Ok(u) = p0_updates.recv_timeout(Duration::from_millis(500)) {
            p0_all.push(u.view);
        } else {
            break;
        }
    }
    while let Ok(u) = p1_updates.try_recv() {
        p1_all.push(u.view);
    }
    (p0_all, p1_all)
}

fn has_seven_showcase_beat(views: &[PlayerView]) -> bool {
    views
        .iter()
        .filter_map(|v| v.script.as_ref())
        .flat_map(|s| s.beats.iter())
        .any(|beat| {
            beat.iter()
                .filter(|step| matches!(step.act, Act::Showcase { .. }))
                .count()
                == 7
        })
}

/// In a shared-deck game, a qualifying hand before a regular mulligan offers "Free Mulligan" on
/// the mode mulligan prompt, while a non-qualifying hand does not. Taking a free mulligan reveals
/// all seven cards in one showcase beat to the opponent both when the opponent declares first and
/// when the opponent declares second (where the hand is immediately returned and shuffled).
#[test]
fn free_mulligan_button_and_revealed_hand_showcase() {
    use beryl_card_names::names::{COUNTERSPELL, ISLAND, REPRIEVE, STOCK_UP, THIRST_FOR_DISCOVERY};

    // Case 1: P0 takes a free mulligan first; P1's script showcases all 7 cards in one beat.
    let (_, p1_views) = play_free_mulligan_board(
        &[
            ISLAND,
            ISLAND,
            ISLAND,
            COUNTERSPELL,
            ISLAND,
            ISLAND,
            ISLAND,
            STOCK_UP,
            ISLAND,
            ISLAND,
            ISLAND,
            REPRIEVE,
            COUNTERSPELL,
            THIRST_FOR_DISCOVERY,
            ISLAND,
            ISLAND,
            ISLAND,
            COUNTERSPELL,
            STOCK_UP,
            REPRIEVE,
            THIRST_FOR_DISCOVERY,
        ],
        true,
    );
    assert!(
        has_seven_showcase_beat(&p1_views),
        "P1 should see all 7 of P0's free-mulligan cards in one showcase beat"
    );

    // Case 2: P0 keeps, then P1 takes a free mulligan second (immediately shuffled); P0's script
    // still showcases all 7 of P1's revealed cards in one beat.
    let (p0_views, _) = play_free_mulligan_board(
        &[
            ISLAND,
            ISLAND,
            COUNTERSPELL,
            ISLAND,
            ISLAND,
            ISLAND,
            STOCK_UP,
            ISLAND,
            ISLAND,
            ISLAND,
            REPRIEVE,
            ISLAND,
            THIRST_FOR_DISCOVERY,
            COUNTERSPELL,
            ISLAND,
            ISLAND,
            ISLAND,
            COUNTERSPELL,
            STOCK_UP,
            REPRIEVE,
            THIRST_FOR_DISCOVERY,
        ],
        false,
    );
    assert!(
        has_seven_showcase_beat(&p0_views),
        "P0 should see all 7 of P1's free-mulligan cards in one showcase beat even when P1 declares second"
    );
}
