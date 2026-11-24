//! In a capture, each update has card lines and a protocol payload for details the tree omits.
//! The replay driver also needs the other fixture messages, such as replies and card records.

use std::fmt::Write;

use beryl_protocol::GameUpdate;
use beryl_protocol::ipc::{CARD_RECORDS, FixtureMessage, GAME_UPDATE};
use beryl_protocol::protocol::{ObjectView, Side, ZoneName};
use game_table::table::Place;
use serde_json::Value;

use super::table::{placeholder_records, read_capture_card};

pub(super) fn read_replay(lines: &[(usize, &str)]) -> Result<Vec<FixtureMessage>, String> {
    let placeholders = lines
        .first()
        .is_some_and(|(_, line)| *line == "placeholder-faces");
    let mut messages = read(&lines[usize::from(placeholders)..])?;
    if placeholders {
        let mut views = Vec::new();
        for message in &messages {
            if let FixtureMessage::Event { event, payload } = message
                && event == GAME_UPDATE
            {
                let update: GameUpdate =
                    serde_json::from_value(payload.clone()).map_err(|error| error.to_string())?;
                views.push(update.view);
            }
        }
        messages.insert(
            0,
            FixtureMessage::response(CARD_RECORDS, &placeholder_records(&views)),
        );
    }
    Ok(messages)
}

pub(super) fn read(lines: &[(usize, &str)]) -> Result<Vec<FixtureMessage>, String> {
    let mut messages = Vec::new();
    let mut current: Option<(Value, Vec<Value>)> = None;
    for &(index, line) in lines {
        let why = |error| format!("line {index}: {error}");
        if let Some(text) = line.strip_prefix("card ") {
            let (_, cards) = current
                .as_mut()
                .ok_or_else(|| why("a card must follow a view".to_owned()))?;
            cards.push(read_capture_card(text).map_err(why)?);
            continue;
        }
        finish(&mut messages, current.take());
        if let Some(text) = line.strip_prefix("view ") {
            let payload: Value =
                serde_json::from_str(text).map_err(|error| why(error.to_string()))?;
            if !payload["view"].is_object() {
                return Err(why("a view line must hold a game update".to_owned()));
            }
            current = Some((payload, Vec::new()));
        } else if let Some(text) = line.strip_prefix("message ") {
            messages.push(serde_json::from_str(text).map_err(|error| why(error.to_string()))?);
        } else {
            return Err(why("write a view, card, or message line".to_owned()));
        }
    }
    finish(&mut messages, current);
    Ok(messages)
}

fn finish(messages: &mut Vec<FixtureMessage>, view: Option<(Value, Vec<Value>)>) {
    if let Some((mut payload, objects)) = view {
        payload["view"]["objects"] = Value::Array(objects);
        messages.push(FixtureMessage::Event {
            event: GAME_UPDATE.to_owned(),
            payload,
        });
    }
}

pub(super) fn write(text: &mut String, message: &FixtureMessage) -> Result<(), serde_json::Error> {
    if let FixtureMessage::Event { event, payload } = message
        && event == GAME_UPDATE
        && let Some(objects) = payload["view"]["objects"].as_array()
    {
        let mut view = payload.clone();
        view["view"]
            .as_object_mut()
            .expect("an object with cards")
            .remove("objects");
        let _ = writeln!(text, "view {}", serde_json::to_string(&view)?);
        for object in objects {
            write_card(text, object)?;
        }
    } else {
        let _ = writeln!(text, "message {}", serde_json::to_string(message)?);
    }
    Ok(())
}

fn write_card(text: &mut String, object: &Value) -> Result<(), serde_json::Error> {
    let card: ObjectView = serde_json::from_value(object.clone())?;
    let side = if card.zone == ZoneName::Battlefield {
        card.controller
    } else {
        card.zone_side.unwrap_or(card.owner)
    };
    let row = (card.zone == ZoneName::Battlefield).then(|| game_table::table::row_of(&card));
    let place = if card.zone == ZoneName::Stack && side == Side::Opponent {
        "stack.theirs"
    } else {
        Place::Card {
            zone: card.zone,
            side,
            row,
        }
        .label()
    };
    let _ = write!(text, "card");
    if let Some(name) = &card.name {
        let _ = write!(
            text,
            " \"{}\"",
            name.replace('\\', "\\\\").replace('"', "\\\"")
        );
    }
    let _ = write!(text, " #{} {place}", card.slot.0);
    if card.tapped {
        let _ = write!(text, " tapped");
    }
    let mut extra = object
        .as_object()
        .expect("a card record is an object")
        .clone();
    for key in ["slot", "name", "zone", "tapped"] {
        extra.remove(key);
    }
    let _ = writeln!(text, " | {}", serde_json::to_string(&extra)?);
    Ok(())
}
