//! The copyable card lines and protocol views of an authored table story.

#[cfg(feature = "stories")]
use std::collections::{BTreeMap, BTreeSet};

use beryl_protocol::PlayerView;
use beryl_protocol::card_records::{CardEntry, CardFace, CardImages, CardRecords};
#[cfg(feature = "stories")]
use beryl_protocol::ipc::{CARD_RECORDS, FixtureMessage};
#[cfg(feature = "stories")]
use beryl_protocol::protocol::{
    CardType, Facing, ObjectId, PlayerState, RowName, SharedPiles, StackItem, Status,
};
use beryl_protocol::protocol::{PowerToughness, Side, ZoneName};
#[cfg(feature = "stories")]
use beryl_protocol::script::{Reset, Script, Table, replay, time_script};
#[cfg(feature = "stories")]
use beryl_protocol::testing::{self, ViewBuilder};
#[cfg(feature = "stories")]
use beryl_protocol::{GameUpdate, SessionId};
use game_hud::labels::GAME_LABELS;
use game_table::table::Place;
use game_table::table::labels::{PHASED_OUT, PICKED, TAPPED};
use kit_frame::frame::StateBits;
use kit_probe::agent::line::{parse_states, quoted_name};

/// The initial view and the view that arrives with each successive script.
#[cfg(feature = "stories")]
#[derive(Clone, Debug, PartialEq)]
pub struct TableStory {
    pub views: Vec<PlayerView>,
    pub actions: Vec<String>,
    pub feature: Option<String>,
}

#[cfg(feature = "stories")]
impl TableStory {
    pub(super) fn parse(lines: &[(usize, &str)], feature: Option<&str>) -> Result<Self, String> {
        let mut builder = ViewBuilder::new();
        let mut slots = Slots::default();
        let mut life = [20; 2];
        let mut stack = Stack::default();
        let mut decision = None;
        let mut handling = None;
        let mut scripts = Vec::new();
        let mut neutral = false;
        let mut manual = false;
        let mut shared = false;
        let mut actions = Vec::new();
        let mut battlefield_order = false;
        let mut lines = lines.iter();

        while let Some(&(index, line)) = lines.next() {
            let (kind, value) = line.split_once(' ').unwrap_or((line, ""));
            let why = |error| format!("line {index}: {error}");
            match kind {
                "card" => {
                    let card = Card::parse(value).map_err(why)?;
                    add_card(&card, &mut builder, &mut slots, &mut stack).map_err(why)?;
                }
                "first-slot" if slots.ids.is_empty() => {
                    slots.next = u32::from(value.parse::<u16>().map_err(|_| {
                        why("the first slot must be between 0 and 65535".to_owned())
                    })?);
                }
                "repeat" => {
                    let count = repeat_count(value).map_err(why)?;
                    let cards = repeat_cards(&mut lines).map_err(why)?;
                    for _ in 0..count {
                        for card in &cards {
                            add_card(card, &mut builder, &mut slots, &mut stack).map_err(why)?;
                        }
                    }
                }
                "ability" => stack.ability(value).map_err(why)?,
                "spell" => stack.spell(value).map_err(why)?,
                "life" => read_life(&mut life, value).map_err(why)?,
                "neutral" if value.is_empty() => neutral = true,
                "manual" if value.is_empty() => manual = true,
                "shared" if value.is_empty() => shared = true,
                "action" if !value.is_empty() => actions.push(value.to_owned()),
                "battlefield-order" if value.is_empty() => battlefield_order = true,
                "handling" if handling.is_none() => {
                    handling =
                        Some(serde_json::from_str(value).map_err(|error| why(error.to_string()))?);
                }
                "prompt" if decision.is_none() => {
                    decision =
                        Some(serde_json::from_str(value).map_err(|error| why(error.to_string()))?);
                }
                "script" => {
                    let together = value == "together";
                    let source = if value.is_empty() || together {
                        block(&mut lines, "end-script")
                            .map_err(why)?
                            .into_iter()
                            .map(|(_, line)| line)
                            .collect::<Vec<_>>()
                            .join("\n")
                    } else {
                        value.to_owned()
                    };
                    let script =
                        serde_json::from_str(&source).map_err(|error| why(error.to_string()))?;
                    scripts.push((script, together));
                }
                _ => return Err(why(format!("unknown or repeated table input {kind:?}"))),
            }
        }

        let mut start = builder.build();
        slots.apply(&mut start);
        if shared {
            let mut library = std::mem::take(&mut start.players.you.library);
            library.extend(std::mem::take(&mut start.players.opponent.library));
            let mut graveyard = std::mem::take(&mut start.players.you.graveyard);
            graveyard.extend(std::mem::take(&mut start.players.opponent.graveyard));
            for object in &mut start.objects {
                if matches!(object.zone, ZoneName::Library | ZoneName::Graveyard) {
                    object.zone_side = None;
                }
            }
            start.shared = Some(SharedPiles { library, graveyard });
        }
        stack.finish(&mut start)?;
        [start.players.you.life, start.players.opponent.life] = life;
        if manual {
            start.table = Some(testing::table(&start));
        }
        let mut views = play_views(start, scripts, neutral, battlefield_order)?;
        let end = views.last_mut().expect("a table story ends with a view");
        if let Some(decision) = decision {
            end.status = Status::Deciding { decision };
        }
        end.handling = handling;
        Ok(Self {
            views,
            actions,
            feature: feature.map(str::to_owned),
        })
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

#[cfg(feature = "stories")]
fn play_views(
    mut start: PlayerView,
    scripts: Vec<(Script, bool)>,
    neutral: bool,
    battlefield_order: bool,
) -> Result<Vec<PlayerView>, String> {
    if neutral {
        let neutral = PlayerView::neutral(PlayerState::neutral(20), PlayerState::neutral(20));
        start.turn = neutral.turn;
        start.stops = neutral.stops;
    }
    let mut table = replay(
        Table::of(&start),
        start
            .script
            .as_ref()
            .expect("the initial view resets the table"),
    )
    .map_err(|error| format!("script: {error}"))?;
    let mut views = vec![start];
    for (script, together) in scripts {
        let previous = views.last().expect("a table story starts with a view");
        let (mut view, after) = play(previous, table, script, together)?;
        if battlefield_order {
            order_objects(&mut view);
        }
        views.push(view);
        table = after;
    }
    if !neutral && views.len() > 1 {
        views[0].status = Status::Playing;
    }
    Ok(views)
}

#[cfg(feature = "stories")]
fn add_card(
    card: &Card,
    builder: &mut ViewBuilder,
    slots: &mut Slots,
    stack: &mut Stack,
) -> Result<(), String> {
    if card.zone == ZoneName::Stack {
        stack.size += card.count as usize;
    }
    card.add(builder, slots)
}

#[cfg(feature = "stories")]
fn repeat_cards(lines: &mut std::slice::Iter<'_, (usize, &str)>) -> Result<Vec<Card>, String> {
    block(lines, "end-repeat")?
        .into_iter()
        .map(|(index, line)| {
            let card = line
                .strip_prefix("card ")
                .ok_or_else(|| format!("line {index}: a repeat group takes card lines"))?;
            Card::parse(card).map_err(|error| format!("line {index}: {error}"))
        })
        .collect()
}

#[cfg(feature = "stories")]
fn block<'a>(
    lines: &mut std::slice::Iter<'_, (usize, &'a str)>,
    end: &str,
) -> Result<Vec<(usize, &'a str)>, String> {
    let mut group = Vec::new();
    for &(index, line) in lines {
        if line == end {
            return Ok(group);
        }
        group.push((index, line));
    }
    Err(format!("a block needs a closing {end} line"))
}

#[cfg(feature = "stories")]
#[derive(Default)]
struct Stack {
    size: usize,
    abilities: Vec<(usize, u32, StackItem)>,
    spells: Vec<StackItem>,
}

#[cfg(feature = "stories")]
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

    fn spell(&mut self, text: &str) -> Result<(), String> {
        let item = serde_json::from_str(text).map_err(|error| error.to_string())?;
        if !matches!(item, StackItem::Spell { .. }) {
            return Err("a spell line must hold a stack spell".to_owned());
        }
        self.spells.push(item);
        Ok(())
    }

    fn finish(self, view: &mut PlayerView) -> Result<(), String> {
        let mut updated = BTreeSet::new();
        for spell in self.spells {
            let StackItem::Spell { slot, .. } = &spell else {
                unreachable!("only spells are added to this list");
            };
            if !updated.insert(*slot) {
                return Err(format!("more than one spell line for #{}", slot.0));
            }
            let entry = view
                .stack
                .iter_mut()
                .find(|item| matches!(item, StackItem::Spell { slot: id, .. } if id == slot))
                .ok_or_else(|| format!("a spell line needs a stack card with id #{}", slot.0))?;
            *entry = spell;
        }
        let mut reset = Reset::default();
        for (position, id, item) in self.abilities {
            reset.abilities.push(id);
            view.stack.insert(position, item);
        }
        view.script = Some(testing::script(Some(reset), Vec::new(), Vec::new()));
        Ok(())
    }
}

#[cfg(feature = "stories")]
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
#[cfg(feature = "stories")]
fn play(
    start: &PlayerView,
    table: Table,
    mut script: Script,
    together: bool,
) -> Result<(PlayerView, Table), String> {
    if script.reset.is_some() {
        return Err(
            "a table story's script starts from the card lines, so omit its reset".to_owned(),
        );
    }
    if script.timing.is_empty() {
        script.timing = time_script(&script, &table);
    } else if script.timing.len() != script.steps() {
        return Err("script timing must have one entry per step".to_owned());
    }
    if together {
        // Keep durations, phases and effects. Suppress the sequencing between steps.
        for timing in &mut script.timing {
            timing.delay = 0.0;
            timing.stagger = 0.0;
            timing.after.clear();
            timing.leads = false;
            timing.alone = false;
            timing.passes = false;
        }
    }
    let after = replay(table, &script).map_err(|error| format!("script: {error}"))?;
    let mut view = start.clone();
    view.objects = after.objects.values().cloned().collect();
    view.battlefield.clone_from(&after.battlefield);
    view.stack = after.stack.iter().map(|entry| entry.item.clone()).collect();
    for (player, piles) in [
        (&mut view.players.you, &after.you),
        (&mut view.players.opponent, &after.opponent),
    ] {
        player.life = piles.life;
        player.mana = piles.mana;
        player.library.clone_from(&piles.library);
        player.graveyard.clone_from(&piles.graveyard);
        player.named_counters.clone_from(&piles.named_counters);
    }
    view.turn.number = after.turn.number;
    view.turn.own_number = after.turn.own_number;
    view.turn.active = after.turn.active;
    view.turn.step = after.turn.step;
    view.script = Some(script);
    Ok((view, after))
}

/// Keeps the order in which a view hands battlefield objects to the client.
#[cfg(feature = "stories")]
fn order_objects(view: &mut PlayerView) {
    let positions: BTreeMap<_, _> = view
        .battlefield
        .iter()
        .copied()
        .enumerate()
        .map(|(position, slot)| (slot, position))
        .collect();
    view.objects
        .sort_by_key(|card| positions.get(&card.slot).copied().unwrap_or(usize::MAX));
}

struct Card {
    name: Option<String>,
    slot: Option<u16>,
    zone: ZoneName,
    #[cfg(feature = "stories")]
    side: Side,
    #[cfg(feature = "stories")]
    row: Option<RowName>,
    count: u32,
    states: StateBits,
    really: Option<String>,
    note: Option<String>,
    #[cfg(feature = "stories")]
    power_toughness: Option<PowerToughness>,
    #[cfg(feature = "stories")]
    token: bool,
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
        let (zone, side, row) = match Place::from_label(place) {
            Some(Place::Card { zone, side, row }) => (zone, side, row),
            Some(Place::SharedPile(zone)) => (zone, Side::You, None),
            _ => return Err(format!("{place:?} is not a card's zone")),
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
        let (first, rest) = word(tail);
        let (power_toughness, tail) = if let Some(value) = first
            .strip_prefix("stats(")
            .and_then(|s| s.strip_suffix(')'))
        {
            (Some(read_stats(value)?), rest)
        } else {
            (None, tail)
        };
        let (first, rest) = word(tail);
        let (token, tail) = if first == "token" {
            (true, rest)
        } else {
            (false, tail)
        };
        #[cfg(not(feature = "stories"))]
        let _ = (side, power_toughness, token);
        let (states, count) = repeat(tail)?;
        let mut card = Self {
            name,
            slot,
            zone,
            #[cfg(feature = "stories")]
            side,
            #[cfg(feature = "stories")]
            row,
            count,
            states: StateBits::default(),
            really: None,
            note: None,
            #[cfg(feature = "stories")]
            power_toughness,
            #[cfg(feature = "stories")]
            token,
        };
        for state in parse_states(states, &GAME_LABELS)? {
            let bit = match state.as_str() {
                "tapped" => TAPPED,
                "face-down" => StateBits::FACE_DOWN,
                "phased-out" => PHASED_OUT,
                "picked" => PICKED,
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

    #[cfg(feature = "stories")]
    fn add(&self, builder: &mut ViewBuilder, slots: &mut Slots) -> Result<(), String> {
        for index in 0..self.count {
            let slot = slots.add(self.slot.map(|start| u32::from(start) + index))?;
            let name = self.name.as_ref().map(|name| {
                name.replace("{n}", &(index + 1).to_string())
                    .replace("{id:03}", &format!("{:03}", slot.0))
            });
            let card = builder.card(self.zone, self.side, name.as_deref().unwrap_or_default());
            card.name = name;
            card.tapped = self.states.has(TAPPED);
            card.phased_out = self.states.has(PHASED_OUT);
            card.hidden_pick = self.states.has(PICKED);
            card.really.clone_from(&self.really);
            card.note.clone_from(&self.note);
            card.pt = self.power_toughness;
            card.token = self.token;
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

fn read_stats(text: &str) -> Result<PowerToughness, String> {
    let why = || "write card stats as stats(2/2)".to_owned();
    let (power, toughness) = text.split_once('/').ok_or_else(why)?;
    Ok(PowerToughness {
        power: power.parse().map_err(|_| why())?,
        toughness: toughness.parse().map_err(|_| why())?,
    })
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
    Ok((states, repeat_count(count)?))
}

fn repeat_count(text: &str) -> Result<u32, String> {
    text.parse()
        .ok()
        .filter(|&count| (1..=65_536).contains(&count))
        .ok_or_else(|| "a repeat count must be between 1 and 65536".to_owned())
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
    data.insert("tapped".to_owned(), card.states.has(TAPPED).into());
    Ok(data.into())
}

#[cfg(feature = "stories")]
#[derive(Default)]
struct Slots {
    used: BTreeSet<ObjectId>,
    ids: Vec<ObjectId>,
    next: u32,
}

#[cfg(feature = "stories")]
impl Slots {
    fn add(&mut self, id: Option<u32>) -> Result<ObjectId, String> {
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
        Ok(slot)
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
