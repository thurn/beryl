//! The copyable card lines and protocol views of an authored table story.

use std::collections::BTreeSet;

use beryl_protocol::card_records::{CardEntry, CardFace, CardImages, CardRecords};
use beryl_protocol::ipc::{CARD_RECORDS, FixtureMessage};
use beryl_protocol::protocol::{
    CardType, Facing, ObjectId, RowName, Side, StackItem, Status, ZoneName,
};
use beryl_protocol::script::{Reset, Script, Table, replay, time_script};
use beryl_protocol::testing::{self, ViewBuilder};
use beryl_protocol::{GameUpdate, PlayerView, SessionId};
use kit_frame::frame::{Place, StateBits};
use kit_probe::agent::{
    line::{parse_states, quoted_name},
    tree::place_from_word,
};

/// The initial view and, when there is a script, the view that script arrives with.
#[derive(Clone, Debug, PartialEq)]
pub struct TableStory {
    pub views: Vec<PlayerView>,
}

impl TableStory {
    pub(super) fn parse(lines: &[(usize, &str)]) -> Result<Self, String> {
        let mut builder = ViewBuilder::new();
        let mut slots = Slots::default();
        let mut life = [20; 2];
        let mut stack = Stack::default();
        let mut decision = None;
        let mut script = None;

        for &(index, line) in lines {
            let (kind, value) = line.split_once(' ').unwrap_or((line, ""));
            let why = |error| format!("line {index}: {error}");
            match kind {
                "card" => {
                    let card = Card::parse(value).map_err(why)?;
                    if card.zone == ZoneName::Stack {
                        stack.size += card.count as usize;
                    }
                    card.add(&mut builder, &mut slots).map_err(why)?;
                }
                "ability" => stack.ability(value).map_err(why)?,
                "life" => read_life(&mut life, value).map_err(why)?,
                "prompt" if decision.is_none() => {
                    decision =
                        Some(serde_json::from_str(value).map_err(|error| why(error.to_string()))?);
                }
                "script" if script.is_none() => {
                    script =
                        Some(serde_json::from_str(value).map_err(|error| why(error.to_string()))?);
                }
                _ => {
                    return Err(why(
                        "write a card, ability, life, or a single prompt or script line".to_owned(),
                    ));
                }
            }
        }

        let mut start = builder.build();
        slots.apply(&mut start);
        stack.finish(&mut start);
        [start.players.you.life, start.players.opponent.life] = life;
        let status = decision.map(|decision| Status::Deciding { decision });
        let views = if let Some(script) = script {
            let mut end = play(&start, script)?;
            if let Some(status) = status {
                end.status = status;
            }
            start.status = Status::Playing;
            vec![start, end]
        } else {
            if let Some(status) = status {
                start.status = status;
            }
            vec![start]
        };
        Ok(Self { views })
    }

    /// The messages the replay driver needs to play this story using its generated faces.
    #[must_use]
    pub fn messages(&self) -> Vec<FixtureMessage> {
        let mut messages = vec![FixtureMessage::response(
            CARD_RECORDS,
            &placeholder_records(&self.views),
        )];
        messages.extend(self.views.iter().map(|view| {
            FixtureMessage::game_update(&GameUpdate {
                session: SessionId(1),
                view: view.clone(),
            })
        }));
        messages
    }
}

#[derive(Default)]
struct Stack {
    size: usize,
    abilities: Vec<(usize, u32, StackItem)>,
}

impl Stack {
    fn ability(&mut self, text: &str) -> Result<(), String> {
        let (id, item) = text
            .split_once(' ')
            .ok_or("write ability <id> <StackItem JSON>")?;
        let id = id.parse().map_err(|_| "an ability needs a numeric id")?;
        let item: StackItem = serde_json::from_str(item).map_err(|error| error.to_string())?;
        if !matches!(item, StackItem::Ability { .. }) {
            return Err("an ability line must hold a stack ability".to_owned());
        }
        if self.abilities.iter().any(|(_, used, _)| *used == id) {
            return Err(format!("ability id {id} is already used"));
        }
        if item.marker_id().is_some_and(|marker| id != marker) {
            return Err(format!("ability id {id} must match its manual marker"));
        }
        self.abilities.push((self.size, id, item));
        self.size += 1;
        Ok(())
    }

    fn finish(self, view: &mut PlayerView) {
        let mut reset = Reset::default();
        for (position, id, item) in self.abilities {
            reset.abilities.push(id);
            view.stack.insert(position, item);
        }
        view.script = Some(testing::script(Some(reset), Vec::new(), Vec::new()));
    }
}

fn read_life(life: &mut [i32; 2], text: &str) -> Result<(), String> {
    let Some((side, value)) = text.split_once(' ') else {
        return Err("write life yours 20 or life theirs 20".to_owned());
    };
    let index = match side {
        "yours" => 0,
        "theirs" => 1,
        _ => return Err("life takes yours or theirs".to_owned()),
    };
    life[index] = value
        .parse()
        .map_err(|_| "life takes a whole number".to_owned())?;
    Ok(())
}

/// Applies the script with the same table replay used by the session and client.
fn play(start: &PlayerView, mut script: Script) -> Result<PlayerView, String> {
    if script.reset.is_some() {
        return Err(
            "a table story's script starts from the card lines, so omit its reset".to_owned(),
        );
    }
    let initial = start
        .script
        .as_ref()
        .expect("the table story starts with a reset");
    let table = replay(Table::of(start), initial).map_err(|error| format!("script: {error}"))?;
    if script.timing.is_empty() {
        script.timing = time_script(&script, &table);
    } else if script.timing.len() != script.steps() {
        return Err("script timing must have one entry per step".to_owned());
    }
    let after = replay(table, &script).map_err(|error| format!("script: {error}"))?;
    let mut view = start.clone();
    view.objects = after.objects.into_values().collect();
    view.battlefield = after.battlefield;
    view.stack = after.stack.into_iter().map(|entry| entry.item).collect();
    for (player, piles) in [
        (&mut view.players.you, after.you),
        (&mut view.players.opponent, after.opponent),
    ] {
        player.life = piles.life;
        player.mana = piles.mana;
        player.library = piles.library;
        player.graveyard = piles.graveyard;
        player.named_counters = piles.named_counters;
    }
    view.turn.number = after.turn.number;
    view.turn.own_number = after.turn.own_number;
    view.turn.active = after.turn.active;
    view.turn.step = after.turn.step;
    view.script = Some(script);
    Ok(view)
}

struct Card {
    name: Option<String>,
    slot: Option<u16>,
    zone: ZoneName,
    side: Side,
    row: Option<RowName>,
    count: u32,
    states: StateBits,
    really: Option<String>,
    note: Option<String>,
}

impl Card {
    fn parse(mut text: &str) -> Result<Self, String> {
        let name = if let Some(quoted) = text.strip_prefix('"') {
            let (name, rest) = quoted_name(quoted).ok_or("the card name's quote is not closed")?;
            text = rest;
            Some(name)
        } else {
            None
        };
        let (first, rest) = word(text);
        let (slot, place, tail) = if let Some(number) = first.strip_prefix('#') {
            let slot = number
                .parse()
                .map_err(|_| "a card id must be between #0 and #65535")?;
            let (place, tail) = word(rest);
            (Some(slot), place, tail)
        } else {
            (None, first, rest)
        };
        let (place, stack_side) = if place == "stack.theirs" {
            ("stack", Side::Opponent)
        } else {
            (place, Side::You)
        };
        let Some(Place::Card { zone, side, row }) = place_from_word(place) else {
            return Err(format!("{place:?} is not a card's zone"));
        };
        if row.is_some() && zone != ZoneName::Battlefield {
            return Err("only a battlefield place takes a row".to_owned());
        }
        let side = if zone == ZoneName::Stack {
            stack_side
        } else {
            side
        };
        // Pasted tree lines have two geometry words before the states. Placement recomputes them.
        let (first, rest) = word(tail);
        let tail = if first.contains(',') {
            word(rest).1
        } else {
            tail
        };
        let (states, count) = repeat(tail)?;
        let mut card = Self {
            name,
            slot,
            zone,
            side,
            row,
            count,
            states: StateBits::default(),
            really: None,
            note: None,
        };
        for state in parse_states(states)? {
            let bit = match state.as_str() {
                "tapped" => StateBits::TAPPED,
                "face-down" => StateBits::FACE_DOWN,
                "phased-out" => StateBits::PHASED_OUT,
                "picked" => StateBits::PICKED,
                text => {
                    for (prefix, name) in
                        [("copy(\"", &mut card.really), ("note(\"", &mut card.note)]
                    {
                        if let Some(text) = text.strip_prefix(prefix) {
                            *name = quoted_name(text).map(|(name, _)| name);
                        }
                    }
                    continue;
                }
            };
            card.states.0 |= bit;
        }
        Ok(card)
    }

    fn add(self, builder: &mut ViewBuilder, slots: &mut Slots) -> Result<(), String> {
        for index in 0..self.count {
            slots.add(self.slot.map(|start| u32::from(start) + index))?;
            let name = self
                .name
                .as_ref()
                .map(|name| name.replace("{n}", &(index + 1).to_string()));
            let card = builder.card(self.zone, self.side, name.as_deref().unwrap_or_default());
            card.name = name;
            card.tapped = self.states.has(StateBits::TAPPED);
            card.phased_out = self.states.has(StateBits::PHASED_OUT);
            card.hidden_pick = self.states.has(StateBits::PICKED);
            card.really.clone_from(&self.really);
            card.note.clone_from(&self.note);
            if self.states.has(StateBits::FACE_DOWN) {
                card.facing = Facing::Down;
            }
            card.types = match self.row {
                Some(RowName::Lands) => vec![CardType::Land],
                Some(RowName::Creatures) => vec![CardType::Creature],
                Some(RowName::Others) | None => Vec::new(),
            };
        }
        Ok(())
    }
}

fn word(text: &str) -> (&str, &str) {
    text.trim_start()
        .split_once(char::is_whitespace)
        .map_or_else(
            || (text.trim_start(), ""),
            |(word, rest)| (word, rest.trim_start()),
        )
}

fn repeat(text: &str) -> Result<(&str, u32), String> {
    let (states, last) = text.rsplit_once(char::is_whitespace).unwrap_or(("", text));
    let Some(count) = last.strip_prefix('x') else {
        return Ok((text, 1));
    };
    count
        .parse()
        .ok()
        .filter(|&count| (1..=65_536).contains(&count))
        .map(|count| (states, count))
        .ok_or_else(|| "a repeat count is written x1 through x65536".to_owned())
}

pub(super) fn read_capture_card(text: &str) -> Result<serde_json::Value, String> {
    let detail = text
        .strip_prefix('"')
        .and_then(quoted_name)
        .map_or(text, |(_, rest)| rest);
    let separator = detail
        .find(" | ")
        .ok_or("a captured card needs its protocol details after |")?;
    let (line, extra) = text.split_at(text.len() - detail.len() + separator);
    let extra = &extra[3..];
    let card = Card::parse(line)?;
    let slot = card
        .slot
        .filter(|_| card.count == 1)
        .ok_or("a captured card needs a slot and cannot repeat")?;
    let mut data: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(extra).map_err(|error| error.to_string())?;
    data.insert("slot".to_owned(), slot.into());
    data.insert("name".to_owned(), card.name.into());
    data.insert(
        "zone".to_owned(),
        serde_json::to_value(card.zone).map_err(|error| error.to_string())?,
    );
    data.insert(
        "tapped".to_owned(),
        card.states.has(StateBits::TAPPED).into(),
    );
    Ok(data.into())
}

#[derive(Default)]
struct Slots {
    used: BTreeSet<ObjectId>,
    ids: Vec<ObjectId>,
    next: u32,
}

impl Slots {
    fn add(&mut self, id: Option<u32>) -> Result<(), String> {
        let id = id.unwrap_or(self.next);
        let slot = u16::try_from(id)
            .map(ObjectId)
            .map_err(|_| "card ids end at #65535")?;
        if !self.used.insert(slot) {
            return Err(format!("card id #{id} is already used"));
        }
        self.ids.push(slot);
        while u16::try_from(self.next).is_ok_and(|id| self.used.contains(&ObjectId(id))) {
            self.next += 1;
        }
        Ok(())
    }

    fn apply(&self, view: &mut PlayerView) {
        let id = |slot: ObjectId| self.ids[usize::from(slot.0)];
        for object in &mut view.objects {
            object.slot = id(object.slot);
        }
        for slots in [
            &mut view.battlefield,
            &mut view.players.you.library,
            &mut view.players.opponent.library,
            &mut view.players.you.graveyard,
            &mut view.players.opponent.graveyard,
        ] {
            for slot in slots {
                *slot = id(*slot);
            }
        }
        for item in &mut view.stack {
            if let StackItem::Spell { slot, .. } = item {
                *slot = id(*slot);
            }
        }
    }
}

/// Placeholder faces for the cards of `views`, one per name.
#[must_use]
pub fn placeholder_records(views: &[PlayerView]) -> CardRecords {
    let mut records = CardRecords::none();
    for object in views.iter().flat_map(|view| &view.objects) {
        let Some(name) = &object.name else { continue };
        let entries = if object.token {
            &mut records.tokens
        } else {
            &mut records.cards
        };
        entries.entry(name.clone()).or_insert_with(|| CardEntry {
            name: name.clone(),
            scryfall_id: String::new(),
            set: String::new(),
            collector_number: String::new(),
            layout: "normal".to_owned(),
            artist: String::new(),
            faces: vec![CardFace {
                name: name.clone(),
                mana_cost: None,
                type_line: String::new(),
                oracle_text: None,
                power: None,
                toughness: None,
                loyalty: None,
                colors: Vec::new(),
                images: CardImages {
                    art_crop: String::new(),
                    normal: String::new(),
                    large: format!("stress-{}", object.slot.0),
                },
            }],
        });
    }
    records
}
