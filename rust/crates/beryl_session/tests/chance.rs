//! Saves record what chance produced (`beryl_engine::chance`): each hidden card as a player
//! first learns it and each coin flip. A save replays the same under another seed, so it does
//! not depend on the seed, and a record chance cannot reproduce stops the replay.

use std::collections::BTreeSet;

use beryl_cards::CARDS;
use beryl_cards::decks::{VINTAGE_WU_CONTROL, WU_CONTROL};
use beryl_engine::Decklist;
use beryl_session::replay::Entry;
use beryl_session::save::{Act, Described};
use beryl_session::{
    Game, PlayerView, ReplayFile, Seat, SessionError, SessionId, StartGame, Status, StopsSetting,
};

const SESSION: SessionId = SessionId(1);

/// The most decisions of a game played here.
const DECISIONS: usize = 600;

fn owned(deck: &Decklist) -> Vec<(u8, String)> {
    deck.iter().map(|&(n, name)| (n, name.to_owned())).collect()
}

/// Plays a game between `decks` from `seed` to its end, or for [`DECISIONS`] of the human's
/// decisions: the random agent in both seats, answering for the human as a stand-in.
fn played(seed: u64, decks: [&Decklist; 2]) -> (Game, PlayerView) {
    let start = StartGame {
        seed,
        decks: decks.map(owned),
        human: Seat::P0,
        opponent: "random".to_owned(),
        stops: StopsSetting::EXACT,
    };
    let mut game = Game::new(&CARDS, &start, SESSION).unwrap();
    game.set_stand_in("random").unwrap();
    let mut last = None;
    for _ in 0..DECISIONS {
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

/// The log of a save as a later replay must reproduce it: each answer by what it does (its
/// index depends on where the game's cards happen to lie), each other entry whole.
fn meaning(file: &ReplayFile) -> Vec<Result<Option<Described>, Entry>> {
    file.log
        .iter()
        .map(|e| match e {
            Entry::Choice(c) | Entry::Auto(c) => Ok(c.what.clone()),
            e => Err(e.clone()),
        })
        .collect()
}

/// What the human sees of the game, by name rather than by slot: the turn, the life totals,
/// and each object they see.
fn seen(view: &PlayerView) -> (String, [i16; 2], Vec<String>) {
    let mut objects: Vec<String> = view
        .objects
        .iter()
        .map(|o| {
            format!(
                "{:?} {:?} {:?} {:?} tapped {} damage {}",
                o.zone, o.owner, o.controller, o.name, o.tapped, o.damage
            )
        })
        .collect();
    objects.sort();
    (
        format!("{:?}", view.turn),
        [view.players.you.life, view.players.opponent.life],
        objects,
    )
}

/// `file` replayed under another seed: the same log, and the human sees the same.
fn replays_under_another_seed(file: &ReplayFile, view: &PlayerView) {
    let mut other = file.clone();
    other.seed ^= 0x5eed_0000_0000;
    let (again, replayed) = Game::from_replay(&CARDS, &other, SESSION).unwrap();
    assert_eq!(meaning(again.replay()), meaning(file), "seed {}", file.seed);
    assert_eq!(again.replay().chooser, file.chooser);
    assert_eq!(seen(&replayed), seen(view), "seed {}", file.seed);
}

/// The kinds of random event a game's log shows, among those these tests look for.
fn events(file: &ReplayFile) -> BTreeSet<&'static str> {
    let mut kinds = BTreeSet::new();
    let mut burst = (0, 0);
    for entry in &file.log {
        match entry {
            Entry::Flip { .. } => {
                kinds.insert("coin flip");
            }
            Entry::Reveal { player, .. } => {
                burst = if burst.0 == *player + 1 {
                    (burst.0, burst.1 + 1)
                } else {
                    (*player + 1, 1)
                };
                // A library searched shows its owner every card in it at once.
                if burst.1 == 20 {
                    kinds.insert("library search");
                }
            }
            Entry::Choice(c) | Entry::Auto(c) => {
                burst = (0, 0);
                let Some(what) = &c.what else { continue };
                match &what.act {
                    Act::Mulligan => {
                        kinds.insert("mulligan");
                    }
                    Act::Cast { card, .. } if card.name == "Timetwister" => {
                        kinds.insert("shuffle and draw seven");
                    }
                    Act::Cast { card, .. } if card.name == "Elite Spellbinder" => {
                        kinds.insert("look at a hand");
                    }
                    Act::Cast { card, .. } if card.name == "Stock Up" => {
                        kinds.insert("look at a library's top");
                    }
                    // Its second loyalty ability is Brainstorm's.
                    Act::Activate { card, ability }
                        if card.name == "Jace, the Mind Sculptor" && ability.n == 1 =>
                    {
                        kinds.insert("brainstorm");
                    }
                    _ => {}
                }
            }
            _ => burst = (0, 0),
        }
    }
    kinds
}

#[test]
fn a_save_records_each_card_as_it_is_first_seen_and_the_flip_winner() {
    let (game, _) = played(1, [WU_CONTROL, WU_CONTROL]);
    let file = game.replay();
    assert_eq!(file.format_version, 3);
    let first_seen: Vec<&Entry> = file
        .log
        .iter()
        .filter(|e| matches!(e, Entry::Reveal { .. }))
        .collect();
    // Both opening hands at least.
    assert!(first_seen.len() >= 14, "{first_seen:?}");
    assert!(file.chooser.is_some());
    let json = serde_json::to_string(file).unwrap();
    assert_eq!(&serde_json::from_str::<ReplayFile>(&json).unwrap(), file);
}

/// Games of the decks that shuffle, search, look at hands and libraries, mulligan and flip
/// coins each replay the same under another seed: their records alone deal every card seen.
#[test]
fn a_save_replays_the_same_under_another_seed() {
    let mut kinds = BTreeSet::new();
    for seed in 0..12 {
        let (game, view) = played(seed, [VINTAGE_WU_CONTROL, WU_CONTROL]);
        replays_under_another_seed(game.replay(), &view);
        kinds.extend(events(game.replay()));
    }
    let wanted = BTreeSet::from([
        "brainstorm",
        "coin flip",
        "library search",
        "look at a hand",
        "look at a library's top",
        "mulligan",
        "shuffle and draw seven",
    ]);
    assert_eq!(kinds, wanted);
}

#[test]
fn a_recorded_card_no_unseen_card_can_be_stops_the_replay() {
    let (game, _) = played(2, [WU_CONTROL, WU_CONTROL]);
    let mut file = game.replay().clone();
    // A card of which the deck has one, seen first, then recorded as seen again in its
    // owner's next card first seen: the one copy is known by then.
    let reveals: Vec<usize> = (0..file.log.len())
        .filter(|&i| matches!(file.log[i], Entry::Reveal { .. }))
        .collect();
    let (once, again) = reveals
        .windows(2)
        .map(|w| (w[0], w[1]))
        .find(|&(a, b)| match (&file.log[a], &file.log[b]) {
            (
                Entry::Reveal { player, card },
                Entry::Reveal {
                    player: next,
                    card: other,
                },
            ) => player == next && card != other && !["Plains", "Island"].contains(&card.as_str()),
            _ => false,
        })
        .unwrap();
    let Entry::Reveal { card, .. } = file.log[once].clone() else {
        unreachable!("a reveal")
    };
    let Entry::Reveal { card: twin, .. } = &mut file.log[again] else {
        unreachable!("a reveal")
    };
    *twin = card.clone();
    let err = match Game::from_replay(&CARDS, &file, SESSION) {
        Err(SessionError::BadReplay { reason }) => reason,
        other => panic!("replayed: {:?}", other.map(|_| ())),
    };
    assert!(
        err.contains(&card) && err.contains("no card of theirs that no player knows"),
        "{err}"
    );
}

#[test]
fn a_recorded_reveal_that_never_happens_stops_the_replay() {
    let (game, _) = played(3, [WU_CONTROL, WU_CONTROL]);
    let mut file = game.replay().clone();
    // After the opening hands, before the answer that follows them.
    let at = file
        .log
        .iter()
        .enumerate()
        .filter(|(_, e)| matches!(e, Entry::Choice(_)))
        .nth(1)
        .unwrap()
        .0;
    file.log.insert(
        at,
        Entry::Reveal {
            player: 0,
            card: "Island".to_owned(),
        },
    );
    let err = match Game::from_replay(&CARDS, &file, SESSION) {
        Err(SessionError::BadReplay { reason }) => reason,
        other => panic!("replayed: {:?}", other.map(|_| ())),
    };
    assert!(err.contains("never happened"), "{err}");
}

#[test]
fn a_version_2_save_replays_from_its_seed() {
    let (game, view) = played(4, [WU_CONTROL, WU_CONTROL]);
    let mut old = game.replay().clone();
    old.format_version = 2;
    old.chooser = None;
    old.log
        .retain(|e| !matches!(e, Entry::Reveal { .. } | Entry::Flip { .. }));
    let (again, replayed) = Game::from_replay(&CARDS, &old, SESSION).unwrap();
    assert_eq!(replayed, view);
    // It is saved again in the format of today, with the outcomes its seed produced.
    assert_eq!(again.replay().format_version, 3);
    assert_eq!(meaning(again.replay()), meaning(game.replay()));
}
