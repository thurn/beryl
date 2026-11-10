//! Upgrading old saves (architecture §14). A replay file names its format
//! ([`ReplayFile::format_version`]); loading one runs it up a chain of migrations, each
//! upgrading a file of one version to the next as JSON, until it is in the format this build
//! writes ([`FORMAT_VERSION`]). The rest of the session sees only that format.
//!
//! Most migrations are pure rewrites of the JSON. The first two replay the file on this build
//! instead, because what the newer format records (what each answer did, then what chance
//! produced) is not in the older file: version 1 to 2 re-records each answer by what it does,
//! and version 2 to 3 records the cards and coin flips its seed produced. They fail where the
//! old file no longer replays on this build, which is why those formats were fragile.
//!
//! # Policy
//!
//! A change that breaks a save, a change to the format or one that makes an old save replay
//! differently, comes in the same commit as either
//!
//! - a migration: bump [`FORMAT_VERSION`], add the step from the old version to the end of the
//!   chain with a test of its own, and leave the steps before it alone; or
//! - an entry in the breaks list, `data/save-breaks.json` ([`Break`]), for each save of the
//!   frozen corpus that no migration can carry past the change: where it stops matching and
//!   why (a rules fix cites the Comprehensive Rules). The corpus test holds a listed save to
//!   that entry instead of to its end.

use beryl_engine::CardTable;
use serde::Deserialize;
use serde_json::Value;

use crate::replay::{Entry, FORMAT_VERSION, ReplayFile};
use crate::{Game, SessionError, SessionId};

/// One migration: a file of version `n`, upgraded to version `n + 1`.
pub type Step = fn(Value, &'static CardTable) -> Result<Value, SessionError>;

/// The migrations, in order: the first upgrades a version 1 file to version 2. Its length is
/// [`FORMAT_VERSION`] less one.
pub const CHAIN: [Step; FORMAT_VERSION as usize - 1] = [
    v1_to_v2, v2_to_v3, v3_to_v4, v4_to_v5, v5_to_v6, v6_to_v7, v7_to_v8,
];

/// `file`, a replay file of any version this build knows, in the format it writes. `cards`
/// are the cards its games are played with, which the migrations that replay need.
pub fn to_current(file: Value, cards: &'static CardTable) -> Result<ReplayFile, SessionError> {
    serde_json::from_value(upgrade(file, cards, &CHAIN)?).map_err(|e| bad(&e))
}

/// `file` run up `chain`, whose step `i` upgrades version `i + 1`, to the version after its
/// last step. Fails on a version newer than that, a step that fails, or a step that does not
/// leave the file one version newer.
pub fn upgrade(
    mut file: Value,
    cards: &'static CardTable,
    chain: &[Step],
) -> Result<Value, SessionError> {
    let current = chain.len() + 1;
    let mut version = version_of(&file)?;
    if version > current {
        return Err(SessionError::BadReplay {
            reason: format!(
                "the file is version {version}, from a newer build: this one reads up to \
                 {current}"
            ),
        });
    }
    while version < current {
        file = chain[version - 1](file, cards)?;
        let next = version_of(&file)?;
        if next != version + 1 {
            return Err(SessionError::BadReplay {
                reason: format!("the upgrade from version {version} left version {next}"),
            });
        }
        version = next;
    }
    Ok(file)
}

/// The version of `file`: its `formatVersion`, or 1 where it has none.
fn version_of(file: &Value) -> Result<usize, SessionError> {
    match file.get("formatVersion") {
        None => Ok(1),
        Some(v) => v
            .as_u64()
            .and_then(|v| usize::try_from(v).ok())
            .filter(|&v| v >= 1)
            .ok_or_else(|| SessionError::BadReplay {
                reason: format!("no format version {v}"),
            }),
    }
}

fn bad(e: &serde_json::Error) -> SessionError {
    SessionError::BadReplay {
        reason: e.to_string(),
    }
}

/// Version 1 to 2: each answer, recorded by its index alone, replayed on this build and
/// recorded again by what it does ([`crate::save`]).
fn v1_to_v2(file: Value, cards: &'static CardTable) -> Result<Value, SessionError> {
    rerecord(file, cards, 1, false)
}

/// Version 2 to 3: replayed from its seed, with what chance produced recorded: each card a
/// player first learned, each coin flip and who won the flip to play first
/// ([`beryl_engine::chance`]).
fn v2_to_v3(file: Value, cards: &'static CardTable) -> Result<Value, SessionError> {
    rerecord(file, cards, 2, true)
}

/// Version 3 to 4: a version 3 file is a version 4 file without turn-start checkpoints
/// ([`crate::checkpoint`]), which a replay checks only where the record has them.
fn v3_to_v4(mut file: Value, _: &'static CardTable) -> Result<Value, SessionError> {
    set_version(&mut file, 4)?;
    Ok(file)
}

/// Version 4 to 5: version 5 names an object's owner where another player controls it, and
/// whether the other player knows a card in a hand or library. Each object a version 4 answer
/// names is marked as named before version 5 ([`crate::save::Obj::before_v5`]), which replay
/// matches with an object either way, as version 4 did.
fn v4_to_v5(mut file: Value, _: &'static CardTable) -> Result<Value, SessionError> {
    mark_objects(&mut file, "beforeV5");
    set_version(&mut file, 5)?;
    Ok(file)
}

/// Version 5 to 6: version 6 names whether a permanent is a token or summoning sick, its
/// counters, its damage and its place in combat, and which of its source's triggered abilities a trigger is. Each
/// object a version 5 answer names is marked as named before version 6
/// ([`crate::save::Obj::before_v6`]), which replay matches with a permanent whatever they are,
/// as version 5 did; a trigger without its ability matches either of its source's.
fn v5_to_v6(mut file: Value, _: &'static CardTable) -> Result<Value, SessionError> {
    mark_objects(&mut file, "beforeV6");
    set_version(&mut file, 6)?;
    Ok(file)
}

/// Version 6 to 7: mana payment now precedes exile costs. Keep every recorded payment
/// choice, moving it before the same cast's exile choices. A partially selected old cost
/// with no payment chosen yet resumes at that payment decision (see `Game::replay_log`),
/// with the unfinished exile selections restored. Preserve older descriptor compatibility
/// markers instead of re-recording unrelated choices.
fn v6_to_v7(mut file: Value, cards: &'static CardTable) -> Result<Value, SessionError> {
    fn cost(entry: &Value) -> Option<&Value> {
        let answer = entry.get("choice").or_else(|| entry.get("auto"))?;
        matches!(
            answer["kind"].as_str(),
            Some("exileCost" | "paymentPlan" | "manaPayment")
        )
        .then_some(answer)
    }
    let mut unfinished = None;
    if let Some(log) = file.get_mut("log").and_then(Value::as_array_mut) {
        let mut start = 0;
        while start < log.len() {
            let Some(first) = cost(&log[start]) else {
                start += 1;
                continue;
            };
            let same_cast = |entry: &Value| {
                cost(entry).is_some_and(|answer| {
                    ["player", "turn", "step"]
                        .iter()
                        .all(|key| answer[key] == first[key])
                })
            };
            let end = start + log[start..].iter().take_while(|e| same_cast(e)).count();
            let exiles = log[start..end]
                .iter()
                .any(|e| cost(e).unwrap()["kind"] == "exileCost");
            if exiles {
                if end == log.len()
                    && log[start..end]
                        .iter()
                        .all(|e| cost(e).unwrap()["kind"] == "exileCost")
                {
                    unfinished = Some(start);
                }
                // Group boundaries belong to positions, not the cost moved there: an
                // automatic plan after a user exile pick must not join the preceding cast
                // and leave that exile pick as an independent Undo after reordering.
                let groups: Vec<_> = log[start..end]
                    .iter()
                    .map(|e| cost(e).unwrap().get("continuation").cloned())
                    .collect();
                log[start..end].sort_by_key(|e| cost(e).unwrap()["kind"] == "exileCost");
                for (entry, group) in log[start..end].iter_mut().zip(groups) {
                    let key = if entry.get("choice").is_some() {
                        "choice"
                    } else {
                        "auto"
                    };
                    let answer = entry[key].as_object_mut().expect("a cost answer");
                    if let Some(group) = group {
                        answer.insert("continuation".to_owned(), group);
                    } else {
                        answer.remove("continuation");
                    }
                }
            }
            start = end;
        }
    }
    if let Some(start) = unfinished {
        let old: ReplayFile = serde_json::from_value(file.clone()).map_err(|e| bad(&e))?;
        let (_, view) = Game::replay_exactly(cards, &old, SessionId(0))?;
        if matches!(view.status, crate::Status::Deciding { decision }
            if matches!(decision.kind, crate::protocol::Kind::PaymentPlan | crate::protocol::Kind::ManaPayment))
        {
            file["log"]
                .as_array_mut()
                .expect("a cost log")
                .truncate(start);
        }
    }
    set_version(&mut file, 7)?;
    Ok(file)
}

/// Version 7 silently answered singleton waterbend decisions. The replay reconstructs
/// those answers where they fell and records them as continuations in its new log.
fn v7_to_v8(mut file: Value, _: &'static CardTable) -> Result<Value, SessionError> {
    set_version(&mut file, 8)?;
    file["legacyWaterbend"] = true.into();
    Ok(file)
}

/// Marks each object `file`'s answers name, what has a zone and a controller, with `field`.
fn mark_objects(file: &mut Value, field: &str) {
    fn mark(value: &mut Value, field: &str) {
        match value {
            Value::Object(fields) => {
                if fields.contains_key("zone") && fields.contains_key("controller") {
                    fields.insert(field.to_owned(), true.into());
                }
                fields.values_mut().for_each(|v| mark(v, field));
            }
            Value::Array(items) => items.iter_mut().for_each(|v| mark(v, field)),
            _ => {}
        }
    }
    if let Some(log) = file.get_mut("log").and_then(Value::as_array_mut) {
        for entry in log.iter_mut().filter_map(Value::as_object_mut) {
            for kind in ["choice", "auto"] {
                if let Some(answer) = entry.get_mut(kind) {
                    mark(answer, field);
                }
            }
        }
    }
}

/// `file`, of version `from`, replayed on this build and recorded again as version `from + 1`:
/// the log the replay recorded, without what that version lacks (the outcomes of chance unless
/// `chance`, and turn-start checkpoints).
fn rerecord(
    file: Value,
    cards: &'static CardTable,
    from: u8,
    chance: bool,
) -> Result<Value, SessionError> {
    let old: ReplayFile = serde_json::from_value(file).map_err(|e| bad(&e))?;
    if old.format_version != from {
        return Err(SessionError::BadReplay {
            reason: format!(
                "upgrading version {from}, a file of version {}",
                old.format_version
            ),
        });
    }
    let (game, _) = Game::from_replay(cards, &old, SessionId(0)).map_err(|e| match e {
        SessionError::BadReplay { reason } => SessionError::BadReplay {
            reason: format!("version {from} does not replay on this build: {reason}"),
        },
        e => e,
    })?;
    let replayed = game.replay();
    let keep = |e: &&Entry| match e {
        Entry::Reveal { .. } | Entry::Flip { .. } => chance,
        Entry::Checkpoint(_) => false,
        _ => true,
    };
    let new = ReplayFile {
        format_version: from + 1,
        chooser: replayed.chooser.filter(|_| chance),
        log: replayed.log.iter().filter(keep).cloned().collect(),
        ..old
    };
    serde_json::to_value(new).map_err(|e| bad(&e))
}

fn set_version(file: &mut Value, version: u8) -> Result<(), SessionError> {
    let Some(fields) = file.as_object_mut() else {
        return Err(SessionError::BadReplay {
            reason: "the file is not an object".to_owned(),
        });
    };
    fields.insert("formatVersion".to_owned(), version.into());
    Ok(())
}

/// A save of the frozen corpus that no migration carries past a change: the breaks list,
/// `data/save-breaks.json` ([`breaks`]), has one for each.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Break {
    /// The save's file name in the corpus.
    pub save: String,
    /// The index of the first entry of the save's log that no longer replays as recorded.
    pub entry: usize,
    /// Why: what changed, and what the replay does there now.
    pub reason: String,
    /// The rule the change follows, as "CR <number> (<link>): "<quotation>"", where it fixed
    /// the rules.
    #[serde(default)]
    pub cr: Option<String>,
    /// The commit that made the change. An entry is added in that same commit, which cannot
    /// name itself, so this is filled in once it lands.
    #[serde(default)]
    pub commit: Option<String>,
}

const BREAKS: &str = include_str!("../../../../data/save-breaks.json");

/// The breaks list.
pub fn breaks() -> Result<Vec<Break>, serde_json::Error> {
    serde_json::from_str(BREAKS)
}

#[cfg(test)]
mod tests {
    use beryl_cards::CARDS;
    use serde_json::json;

    use super::*;

    /// A synthetic version 1 to 2 for the chain's own test: `name` renamed `title`.
    const RENAME: Step = |mut file, _| {
        let fields = file.as_object_mut().unwrap();
        let name = fields.remove("name").unwrap();
        fields.insert("title".to_owned(), name);
        fields.insert("formatVersion".to_owned(), 2.into());
        Ok(file)
    };

    /// A synthetic version 2 to 3: `turns` added.
    const ADD_TURNS: Step = |mut file, _| {
        let fields = file.as_object_mut().unwrap();
        fields.insert("turns".to_owned(), 0.into());
        fields.insert("formatVersion".to_owned(), 3.into());
        Ok(file)
    };

    /// A step that leaves the version as it was.
    const FORGETS_THE_VERSION: Step = |file, _| Ok(file);

    #[test]
    fn a_file_runs_up_the_chain_from_its_version() {
        let chain: [Step; 2] = [RENAME, ADD_TURNS];
        let v1 = json!({ "name": "x" });
        assert_eq!(
            upgrade(v1, &CARDS, &chain).unwrap(),
            json!({ "formatVersion": 3, "title": "x", "turns": 0 })
        );
        let v2 = json!({ "formatVersion": 2, "title": "y" });
        assert_eq!(
            upgrade(v2, &CARDS, &chain).unwrap(),
            json!({ "formatVersion": 3, "title": "y", "turns": 0 })
        );
        let v3 = json!({ "formatVersion": 3, "title": "z", "turns": 4 });
        assert_eq!(upgrade(v3.clone(), &CARDS, &chain).unwrap(), v3);
    }

    #[test]
    fn a_newer_file_or_a_step_that_skips_a_version_fails() {
        let chain: [Step; 2] = [RENAME, ADD_TURNS];
        let newer = json!({ "formatVersion": 4 });
        let Err(SessionError::BadReplay { reason }) = upgrade(newer, &CARDS, &chain) else {
            panic!("a newer file upgraded");
        };
        assert!(reason.contains("newer build"), "{reason}");
        let chain: [Step; 1] = [FORGETS_THE_VERSION];
        assert!(upgrade(json!({}), &CARDS, &chain).is_err());
        assert!(upgrade(json!({ "formatVersion": 0 }), &CARDS, &chain).is_err());
    }

    #[test]
    fn version_3_to_4_changes_only_the_version() {
        let v3 = json!({ "formatVersion": 3, "seed": 1, "log": [{ "flip": { "player": 0, "won": true } }] });
        let v4 = v3_to_v4(v3.clone(), &CARDS).unwrap();
        let mut expected = v3;
        expected["formatVersion"] = 4.into();
        assert_eq!(v4, expected);
    }

    #[test]
    fn version_4_to_5_marks_the_objects_answers_name() {
        let island = json!({ "name": "Island", "zone": "hand", "controller": 0 });
        let v4 = json!({ "formatVersion": 4, "seed": 1, "log": [
            { "flip": { "player": 0, "won": true } },
            { "choice": { "index": 1, "player": 0, "turn": 1, "step": "precombatMain",
                "kind": "priority", "act": { "act": "playLand", "card": island } } },
        ] });
        let v5 = v4_to_v5(v4.clone(), &CARDS).unwrap();
        let mut expected = v4;
        expected["formatVersion"] = 5.into();
        expected["log"][1]["choice"]["act"]["card"]["beforeV5"] = true.into();
        assert_eq!(v5, expected);
    }

    #[test]
    fn version_5_to_6_marks_the_objects_answers_name() {
        let bear = json!({ "name": "Grizzly Bears", "zone": "battlefield", "controller": 0,
            "beforeV5": true });
        let v5 = json!({ "formatVersion": 5, "seed": 1, "log": [
            { "choice": { "index": 0, "player": 0, "turn": 3, "step": "declareAttackers",
                "kind": "attacker", "act": { "act": "attack", "creature": bear } } },
        ] });
        let v6 = v5_to_v6(v5.clone(), &CARDS).unwrap();
        let mut expected = v5;
        expected["formatVersion"] = 6.into();
        expected["log"][0]["choice"]["act"]["creature"]["beforeV6"] = true.into();
        assert_eq!(v6, expected);
    }

    #[test]
    fn the_breaks_list_parses_with_one_entry_per_save() {
        let breaks = breaks().unwrap();
        let mut saves: Vec<&str> = breaks.iter().map(|b| b.save.as_str()).collect();
        saves.sort_unstable();
        saves.dedup();
        assert_eq!(saves.len(), breaks.len(), "a save listed twice");
        assert!(breaks.iter().all(|b| !b.reason.is_empty()));
    }
}
