//! Saves record each answer by what it does (`beryl_session::save`): a game replays from one
//! though its candidates move, a version 1 save still replays, and where the record and the
//! game part, replay goes on only where the rules leave nothing to choose.

use beryl_cards::CARDS;
use beryl_cards::decks::fuzz_deck;
use beryl_session::replay::Entry;
use beryl_session::save::{self, Act, Chosen, Described, Kind, Obj};
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
    assert_eq!(file.format_version, beryl_session::replay::FORMAT_VERSION);
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
    save::reverse_candidates(true);
    let reversed = Game::from_replay(&CARDS, game.replay(), SESSION);
    save::reverse_candidates(false);
    let (again, replayed) = reversed.unwrap();
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

#[test]
fn a_version_1_save_still_replays() {
    let (game, view, _) = played(4, 30);
    let mut old = game.replay().clone();
    old.commit = None;
    old.chooser = None;
    old.log
        .retain(|e| !matches!(e, Entry::Reveal { .. } | Entry::Flip { .. }));
    for entry in &mut old.log {
        if let Entry::Choice(c) | Entry::Auto(c) = entry {
            c.what = None;
        }
    }
    let mut json: serde_json::Value = serde_json::to_value(&old).unwrap();
    json.as_object_mut().unwrap().remove("formatVersion");
    let text = json.to_string();
    assert!(text.contains("{\"choice\":"), "answers as bare indices");
    let old: ReplayFile = serde_json::from_str(&text).unwrap();
    assert_eq!(old.format_version, 1);
    assert_eq!(replayed(&old).unwrap(), view);
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
        what: Some(Described {
            player: 0,
            turn,
            step,
            kind: Kind::Priority,
            act: Act::Pass,
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
    let Err(SessionError::BadReplay { reason }) = replayed(&file) else {
        panic!("the replay goes on past an answer no candidate does");
    };
    assert!(reason.starts_with("turn "), "{reason}");
    assert!(reason.contains("No Such Card"), "{reason}");
}
