//! Saves record each answer by what it does (`beryl_session::save`): a game replays from one
//! though its candidates move, and where the record and the
//! game part, replay goes on only where the rules leave nothing to choose. A version 1 save
//! upgrades by replaying.

use beryl_cards::CARDS;
use beryl_cards::decks::fuzz_deck;
use beryl_session::migrate;
use beryl_session::perturb::{self, Order, Perturbation};
use beryl_session::replay::{Entry, FORMAT_VERSION};
use beryl_session::save::{Act, Chosen, Described, Kind, Obj};
use beryl_session::{
    CandidateIndex, Game, PlayerView, ReplayFile, Seat, SessionError, SessionId, StartGame, Status,
    StopsSetting,
};

const SESSION: SessionId = SessionId(1);

fn owned(deck: &[(u8, &str)]) -> Vec<(u8, String)> {
    deck.iter().map(|&(n, name)| (n, name.to_owned())).collect()
}

/// A game against the random agent between two fuzz decks drawn from `seed`, the human
/// asked at every priority.
fn start(seed: u64) -> StartGame {
    StartGame {
        seed,
        decks: [owned(&fuzz_deck(seed)), owned(&fuzz_deck(seed ^ 1 << 40))],
        human: Seat::P0,
        opponent: "random".to_owned(),
        stops: StopsSetting::EXACT,
    }
}

/// One of the human's answers: where it went in the log, and among how many candidates.
struct Answered {
    at: usize,
    candidates: usize,
}

/// A game played to the human's `answers`th decision by a fixed script, never the agent's
/// stream; returns it, the view there, and each answer.
fn played(seed: u64, answers: usize) -> (Game, PlayerView, Vec<Answered>) {
    let mut game = Game::new(&CARDS, &start(seed), SESSION).unwrap();
    let mut view = to_human(&mut game);
    let mut log = Vec::new();
    for _ in 0..answers {
        let Status::Deciding { decision } = &view.status else {
            panic!("the game ended early: {:?}", view.status);
        };
        let n = decision.candidates.len();
        log.push(Answered {
            at: game.replay().log.len(),
            candidates: n,
        });
        let index = CandidateIndex(((decision.id.seq * 7 + 3) % n as u32) as u16);
        game.choose(game.answer(decision.id, index).unwrap());
        view = to_human(&mut game);
    }
    (game, view, log)
}

fn to_human(game: &mut Game) -> PlayerView {
    let mut last = None;
    while let Some(view) = game.next_view() {
        last = Some(view);
    }
    last.expect("the game plays on from where it is")
}

fn chosen(entry: &Entry) -> &Chosen {
    match entry {
        Entry::Choice(c) => c,
        entry => panic!("not an answer: {entry:?}"),
    }
}

fn replayed(file: &ReplayFile) -> Result<PlayerView, SessionError> {
    Game::from_replay(&CARDS, file, SESSION).map(|(_, view)| view)
}

#[test]
fn a_save_describes_each_answer_by_what_it_does() {
    let (game, _, _) = played(3, 30);
    let file = game.replay();
    assert_eq!(file.format_version, FORMAT_VERSION);
    let mut kinds = std::collections::HashSet::new();
    for entry in &file.log {
        if let Entry::Choice(c) | Entry::Auto(c) = entry {
            let what = c.what.as_ref().expect("every answer says what it does");
            kinds.insert(format!("{:?}", what.kind));
        }
    }
    assert!(kinds.len() > 2, "{kinds:?}");
    let json = serde_json::to_string(file).unwrap();
    assert_eq!(&serde_json::from_str::<ReplayFile>(&json).unwrap(), file);
}

#[test]
fn a_save_replays_the_same_with_its_candidates_listed_the_other_way() {
    let (game, view, _) = played(3, 30);
    let reversed = Perturbation {
        order: Order::Reversed,
        ..Perturbation::default()
    };
    let (again, replayed) = perturb::with(reversed, || {
        Game::from_replay(&CARDS, game.replay(), SESSION)
    })
    .unwrap();
    // The same answers, though identical objects may have traded places.
    let acts = |file: &ReplayFile| -> Vec<Option<Described>> {
        file.log
            .iter()
            .filter_map(|e| match e {
                Entry::Choice(c) | Entry::Auto(c) => Some(c.what.clone()),
                _ => None,
            })
            .collect()
    };
    assert_eq!(acts(again.replay()), acts(game.replay()));
    assert_eq!(replayed.turn, view.turn);
    assert_eq!(
        (replayed.players.you.life, replayed.players.opponent.life),
        (view.players.you.life, view.players.opponent.life)
    );
}

/// `file` as version 1 recorded it: each answer by its index alone, nothing chance produced,
/// no checkpoints and no version.
fn version_1(file: &ReplayFile) -> serde_json::Value {
    let mut old = file.clone();
    old.commit = None;
    old.chooser = None;
    old.log.retain(|e| {
        !matches!(
            e,
            Entry::Reveal { .. } | Entry::Flip { .. } | Entry::Checkpoint(_)
        )
    });
    for entry in &mut old.log {
        if let Entry::Choice(c) | Entry::Auto(c) = entry {
            c.what = None;
        }
    }
    let mut json = serde_json::to_value(&old).unwrap();
    json.as_object_mut().unwrap().remove("formatVersion");
    assert!(
        json.to_string().contains("{\"choice\":"),
        "answers as bare indices"
    );
    json
}

/// `file`'s log without what `keep` rejects.
fn only(file: &ReplayFile, keep: impl Fn(&Entry) -> bool) -> Vec<Entry> {
    file.log.iter().filter(|e| keep(e)).cloned().collect()
}

#[test]
fn a_version_1_save_upgrades_to_version_2_by_replaying() {
    for (seed, answers) in [(4, 30), (6, 60)] {
        let (game, view, _) = played(seed, answers);
        let old = version_1(game.replay());
        let v1: ReplayFile = serde_json::from_value(old.clone()).unwrap();
        assert_eq!(v1.format_version, 1);
        assert_eq!(replayed(&v1).unwrap(), view);
        let v2: ReplayFile =
            serde_json::from_value(migrate::CHAIN[0](old, &CARDS).unwrap()).unwrap();
        assert_eq!(v2.format_version, 2);
        assert_eq!(v2.chooser, None);
        // Each answer again, by what it does.
        assert_eq!(
            v2.log,
            only(game.replay(), |e| !matches!(
                e,
                Entry::Reveal { .. } | Entry::Flip { .. } | Entry::Checkpoint(_)
            ))
        );
        assert_eq!(replayed(&v2).unwrap(), view);
    }
}

#[test]
fn a_loaded_version_1_save_is_in_the_format_of_today() {
    let (game, view, _) = played(4, 30);
    let dir = std::env::temp_dir().join(format!("beryl-migrate-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("v1.json");
    std::fs::write(&path, version_1(game.replay()).to_string()).unwrap();
    let loaded = ReplayFile::load(&path);
    std::fs::remove_dir_all(&dir).unwrap();
    let loaded = loaded.unwrap();
    assert_eq!(loaded.format_version, FORMAT_VERSION);
    assert_eq!(loaded.chooser, game.replay().chooser);
    // Re-recorded on this build, with the objects answers name marked as named before
    // versions 5 and 6, as every version 4 file's are.
    assert_eq!(
        loaded.log,
        before_v5(&only(game.replay(), |e| !matches!(e, Entry::Checkpoint(_))))
    );
    assert_eq!(replayed(&loaded).unwrap(), view);
}

/// `log` with each object its answers name marked as named before versions 5 and 6
/// ([`Obj::before_v5`], [`Obj::before_v6`]), as a version 4 file's are once loaded.
fn before_v5(log: &[Entry]) -> Vec<Entry> {
    fn mark(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(fields) => {
                if fields.contains_key("zone") && fields.contains_key("controller") {
                    fields.insert("beforeV5".to_owned(), true.into());
                    fields.insert("beforeV6".to_owned(), true.into());
                }
                fields.values_mut().for_each(mark);
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(mark),
            _ => {}
        }
    }
    log.iter()
        .map(|e| {
            let mut v = serde_json::to_value(e).unwrap();
            if matches!(e, Entry::Choice(_) | Entry::Auto(_)) {
                mark(&mut v);
            }
            serde_json::from_value(v).unwrap()
        })
        .collect()
}

#[test]
fn a_version_1_save_that_no_longer_replays_does_not_upgrade() {
    let (game, _, _) = played(4, 30);
    let mut old = version_1(game.replay());
    for entry in old["log"].as_array_mut().unwrap() {
        if let Some(index) = entry.get_mut("choice") {
            *index = 900.into();
        }
    }
    let Err(SessionError::BadReplay { reason }) = migrate::to_current(old, &CARDS) else {
        panic!("a version 1 save that does not replay upgraded");
    };
    assert!(reason.contains("version 1 does not replay"), "{reason}");
}

#[test]
fn a_decision_the_record_lacks_is_answered_when_it_has_one_candidate() {
    let (game, view, answers) = played(5, 40);
    // Not the last answer, where the record would simply end sooner.
    let lone = answers[..answers.len() - 1]
        .iter()
        .rev()
        .find(|a| a.candidates == 1)
        .expect("an exact stop with only a pass");
    let mut file = game.replay().clone();
    file.log.remove(lone.at);
    assert_eq!(replayed(&file).unwrap(), view);
}

#[test]
fn a_priority_the_record_lacks_before_its_next_step_is_passed() {
    let (game, view, answers) = played(8, 60);
    let file = game.replay();
    // A pass that could have done something else, the human's last in its step.
    let next_when = |at: usize| {
        file.log[at + 1..].iter().find_map(|e| match e {
            Entry::Choice(c) => c.when(),
            _ => None,
        })
    };
    let pass = answers
        .iter()
        .find(|a| {
            let c = chosen(&file.log[a.at]);
            a.candidates > 1
                && c.is_priority_pass()
                && next_when(a.at).is_some_and(|next| Some(next) > c.when())
        })
        .expect("a pass with something to cast");
    let mut file = file.clone();
    file.log.remove(pass.at);
    assert_eq!(replayed(&file).unwrap(), view);
}

#[test]
fn a_recorded_pass_where_no_priority_is_open_is_skipped() {
    let (game, view, answers) = played(6, 60);
    let file = game.replay();
    let asked = answers
        .iter()
        .find(|a| {
            a.candidates > 1
                && chosen(&file.log[a.at])
                    .what
                    .as_ref()
                    .is_some_and(|w| w.kind != Kind::Priority)
        })
        .expect("a question other than priority");
    let (turn, step) = chosen(&file.log[asked.at]).when().unwrap();
    let mut file = file.clone();
    let pass = Chosen {
        index: 0,
        continuation: false,
        what: Some(Described {
            player: 0,
            turn,
            step,
            kind: Kind::Priority,
            act: Act::Pass,
            nth: None,
        }),
    };
    file.log.insert(asked.at, Entry::Choice(pass));
    assert_eq!(replayed(&file).unwrap(), view);
}

#[test]
fn a_recorded_halt_the_game_no_longer_makes_is_skipped() {
    let (game, view, answers) = played(7, 20);
    let mut file = game.replay().clone();
    file.log.insert(answers[10].at, Entry::Halt);
    assert_eq!(replayed(&file).unwrap(), view);
}

#[test]
fn an_answer_no_candidate_does_stops_the_replay_naming_the_step() {
    let (game, _, answers) = played(3, 30);
    let mut file = game.replay().clone();
    let cast = answers
        .iter()
        .find(|a| {
            matches!(
                chosen(&file.log[a.at]).what,
                Some(Described {
                    act: Act::PlayLand { .. } | Act::Cast { .. },
                    ..
                })
            )
        })
        .expect("the human plays a card");
    let Entry::Choice(Chosen {
        what:
            Some(Described {
                act: Act::PlayLand { card } | Act::Cast { card, .. },
                ..
            }),
        ..
    }) = &mut file.log[cast.at]
    else {
        unreachable!("found above");
    };
    *card = Obj {
        name: "No Such Card".to_owned(),
        ..card.clone()
    };
    // With no turn's beginning to rebuild from (`tests/checkpoints.rs`).
    file.log.retain(|e| !matches!(e, Entry::Checkpoint(_)));
    let Err(SessionError::BadReplay { reason }) = replayed(&file) else {
        panic!("the replay goes on past an answer no candidate does");
    };
    assert!(reason.starts_with("turn "), "{reason}");
    assert!(reason.contains("No Such Card"), "{reason}");
}
