//! The frozen save corpus (`beryl_session::corpus`): every save under `tests/saves/` still
//! replays to its end, matches each board it recorded as a turn began, and ends as it ended;
//! a save on the breaks list, to its listed entry. `just freeze-saves` adds saves.

use std::path::{Path, PathBuf};
use std::thread;

use beryl_cards::CARDS;
use beryl_session::ReplayFile;
use beryl_session::corpus::{self, ENDINGS, Ending, Failure, Level, Outcome};
use beryl_session::migrate::{self, Break};
use beryl_session::replay::Entry;
use beryl_session::save::Act;

fn dir() -> PathBuf {
    let crate_dir = std::env::var_os("CARGO_MANIFEST_DIR").expect("run under cargo");
    Path::new(&crate_dir).join("tests/saves")
}

/// The names of the saves in the corpus.
fn saves() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|name| name != ENDINGS)
        .collect();
    names.sort_unstable();
    names
}

#[test]
fn every_frozen_save_still_plays() {
    let endings = corpus::endings(&dir()).unwrap();
    let saves = saves();
    assert!(!saves.is_empty(), "no saves in {}", dir().display());
    assert_eq!(
        saves,
        endings.keys().cloned().collect::<Vec<_>>(),
        "the saves and {ENDINGS} name different files"
    );
    let breaks = migrate::breaks().unwrap();
    for b in &breaks {
        assert!(
            endings.contains_key(&b.save),
            "the breaks list names {}, no save",
            b.save
        );
    }
    // Each save on its own thread: replaying is the corpus's whole cost.
    let failures: Vec<String> = thread::scope(|scope| {
        let checks: Vec<_> = saves
            .iter()
            .map(|name| {
                let ending = &endings[name];
                let listed = breaks.iter().find(|b| &b.save == name);
                scope.spawn(move || {
                    let file = corpus::load(&CARDS, &dir().join(name)).map_err(|e| e.clone())?;
                    corpus::check_listed(&CARDS, &file, ending, listed).map_err(|f| f.to_string())
                })
            })
            .collect();
        saves
            .iter()
            .zip(checks)
            .filter_map(|(name, check)| match check.join().unwrap() {
                Ok(()) => None,
                Err(e) => Some(format!("{name}: {e}")),
            })
            .collect()
    });
    assert!(
        failures.is_empty(),
        "saves that no longer play as frozen (add a migration or a breaks entry, \
         beryl_session::migrate):\n{}",
        failures.join("\n")
    );
}

/// The smallest save of the corpus that is not on the breaks list and was played to its end
/// over four turns or more, and how it ended: one to edit.
fn smallest() -> (String, ReplayFile, Ending) {
    let endings = corpus::endings(&dir()).unwrap();
    let breaks = migrate::breaks().unwrap();
    let mut names = saves();
    names.sort_by_key(|name| std::fs::metadata(dir().join(name)).unwrap().len());
    names
        .into_iter()
        .filter(|name| breaks.iter().all(|b| &b.save != name))
        .map(|name| {
            let file = corpus::load(&CARDS, &dir().join(&name)).unwrap();
            let ending = endings[&name];
            (name, file, ending)
        })
        .find(|(_, _, ending)| ending.outcome != Outcome::Unfinished && ending.turn >= 4)
        .expect("a save to edit")
}

/// The index of the first land played or spell cast after `from` in `file`'s log, and that
/// play renamed to a card no deck has.
fn impossible(file: &ReplayFile, from: usize) -> (usize, ReplayFile) {
    let mut broken = file.clone();
    let (at, card) = broken
        .log
        .iter_mut()
        .enumerate()
        .skip(from)
        .find_map(|(i, e)| match e {
            Entry::Choice(c) => match &mut c.what.as_mut()?.act {
                Act::PlayLand { card } | Act::Cast { card, .. } => Some((i, card)),
                _ => None,
            },
            _ => None,
        })
        .expect("a card played");
    "No Such Card".clone_into(&mut card.name);
    (at, broken)
}

fn failure(result: Result<impl std::fmt::Debug, Failure>) -> Failure {
    result.expect_err("an edited save still plays")
}

#[test]
fn an_edited_save_fails_at_the_level_it_breaks_saying_where() {
    let (_, file, ending) = smallest();
    assert_eq!(corpus::check(&CARDS, &file, Some(&ending)), Ok(ending));

    // Level 1: an answer no candidate gives.
    let (_, unplayable) = impossible(&file, 0);
    let f = failure(corpus::check(&CARDS, &unplayable, Some(&ending)));
    assert_eq!(f.level, Level::Replays, "{f}");
    assert!(f.reason.contains("No Such Card"), "{f}");
    assert!(f.to_string().starts_with("level 1"), "{f}");

    // Level 2: a board that is not the one the game reaches as its turn begins.
    let mut misboarded = file.clone();
    let turn = misboarded
        .log
        .iter_mut()
        .filter_map(|e| match e {
            Entry::Checkpoint(c) => Some(c),
            _ => None,
        })
        .nth(3)
        .map(|c| {
            c.players[0].life += 1;
            c.turn
        })
        .expect("a fourth turn");
    let f = failure(corpus::check(&CARDS, &misboarded, Some(&ending)));
    assert_eq!(f.level, Level::Checkpoints, "{f}");
    assert!(f.reason.contains(&format!("turn {turn}")), "{f}");

    // Level 3: an ending the game no longer reaches.
    let other = Ending {
        turn: ending.turn + 1,
        ..ending
    };
    let f = failure(corpus::check(&CARDS, &file, Some(&other)));
    assert_eq!(f.level, Level::Ending, "{f}");
    assert!(f.reason.contains(&other.to_string()), "{f}");
    assert!(f.reason.contains(&ending.to_string()), "{f}");
}

#[test]
fn a_listed_save_is_held_to_its_entry() {
    let (name, file, ending) = smallest();
    let (at, broken) = impossible(&file, file.log.len() / 2);
    let listed = |entry| Break {
        save: name.clone(),
        entry,
        reason: "a test".to_owned(),
        cr: None,
        commit: None,
    };
    assert_eq!(
        corpus::check_listed(&CARDS, &broken, &ending, Some(&listed(at))),
        Ok(())
    );
    // Held past where it breaks.
    let f = failure(corpus::check_listed(
        &CARDS,
        &broken,
        &ending,
        Some(&listed(at + 1)),
    ));
    assert_eq!(f.level, Level::Replays, "{f}");
    // A save that plays to its end does not belong on the list.
    let f = failure(corpus::check_listed(
        &CARDS,
        &file,
        &ending,
        Some(&listed(at)),
    ));
    assert!(f.reason.contains("breaks list"), "{f}");
}
