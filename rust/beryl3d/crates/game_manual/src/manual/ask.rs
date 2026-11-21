//! The manual table's ask prompt (docs/manual-multiplayer.md, "Prompts for numbers and
//! text"): a command that needs a number, a text or a power and toughness opens a typed field
//! in the dock at the lower right, where engine games ask for a value, with a minus and a plus
//! beside a number, Cancel, and a button that says what will happen ("Draw 3"). The field opens
//! on the likeliest value, selected, so that typing replaces it; Return confirms and Escape
//! cancels. A value typed with a leading + or − changes the current value where there is one,
//! so "+3" in the life prompt gains three life. This is the web's `AskPrompt`
//! (`ui/src/table/AskPrompt.tsx`), with `parse`, `parse_stats` and the bounds of `ask.ts` and
//! the per-command starts and labels of `TableCommands.run` (`ui/src/table/TableMenu.tsx`).

use crate::features::ask::Message as AskMessage;
use beryl_protocol::PlayerView;
use beryl_protocol::protocol::{Answer, Side, TableAction, TableObjectRef, ZoneName};
use glam::Vec2;
use serde_json::Value;

use super::Target;
use super::commands::{self, Asks, COMMANDS, Command, Values};
use game_hud::hud::placement_scene::PlacementScene;
use game_table::profiles::Profile;
use kit_base::tokens::{color, size, space};
use kit_ui::element::{Align, Anchor, Element, column, row};
use kit_ui::input::Window;
use kit_ui::widgets::{FieldState, Fields, button, field, primary};

/// The most and least a stat or a life total may be (`MAX_INT` and `MIN_INT` in
/// `ui/src/table/cardState.ts`).
const MAX_INT: i64 = i32::MAX as i64;
const MIN_INT: i64 = i32::MIN as i64;

/// What the prompt reads from its field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// An integer from `min` to `max`, which a signed entry changes from `current` if given.
    Number {
        min: i64,
        max: i64,
        current: Option<i64>,
    },
    /// Any text.
    Text,
    /// Power and toughness written `p/t`, each side set or, signed, changed from `current`.
    Stats { current: (i64, i64) },
}

/// Which part of a command the prompt asks for, where a command asks for two things one after
/// the other.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Stage {
    /// The command's only value.
    Only,
    /// A die's number of sides, before how many dice are rolled.
    Sides,
    /// How many dice of `sides` sides are rolled.
    Dice { sides: i64 },
    /// The text of a command that asks for a text and then a number.
    Text,
    /// The number after `text`.
    Count { text: String },
    /// The X of the ability at `row` of the permanent `source` names (`Ask::ability_x`).
    X { source: TableObjectRef, row: usize },
}

/// An open ask prompt: the command it asks for (an index of `COMMANDS`), what the command acts
/// on, what it reads, the field's name and starting text, and which of the command's values it
/// asks for now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ask {
    pub command: u16,
    pub target: Target,
    pub kind: Kind,
    /// The field's name, which is also its label ("Cards to draw").
    pub field: String,
    /// What the field holds when the prompt opens.
    pub start: String,
    stage: Stage,
    /// The word the confirm button starts with, before the number.
    verb: String,
}

/// What a confirmed entry reads as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    /// A number, and how much it changed the current value by if it was typed with a sign.
    Number {
        value: i64,
        change: Option<i64>,
    },
    Text(String),
    Stats(i64, i64),
}

/// What confirming the prompt does: ask the next of the command's values, or send these
/// actions.
#[derive(Clone, Debug, PartialEq)]
pub enum Confirmed {
    Next(Box<Ask>),
    Send(Vec<TableAction>),
    Answer(Answer),
}

/// Reads an integer, as the web's `parse` does: digits after an optional sign, where `−`
/// counts as a minus. With a sign and a `current` value, the number changes it. Returns the
/// value and, for a change, by how much.
#[must_use]
pub fn parse(text: &str, current: Option<i64>) -> Option<(i64, Option<i64>)> {
    let entry = text.trim();
    let entry = entry
        .strip_prefix('−')
        .map_or_else(|| entry.to_owned(), |rest| format!("-{rest}"));
    let digits = entry.strip_prefix(['+', '-']).unwrap_or(&entry);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let amount: i64 = entry.strip_prefix('+').unwrap_or(&entry).parse().ok()?;
    let relative = entry.starts_with(['+', '-']);
    match (relative, current) {
        (true, Some(current)) => Some((current.checked_add(amount)?, Some(amount))),
        _ => Some((amount, None)),
    }
}

/// Reads power and toughness written `p/t`, each side as `parse` reads it against the current
/// value (`parseStats`).
#[must_use]
pub fn parse_stats(text: &str, current: (i64, i64)) -> Option<(i64, i64)> {
    let mut sides = text.split('/');
    let (left, right) = (sides.next()?, sides.next()?);
    if sides.next().is_some() {
        return None;
    }
    let (power, _) = parse(left, Some(current.0))?;
    let (toughness, _) = parse(right, Some(current.1))?;
    Some((power, toughness))
}

impl Ask {
    /// The prompt for the command at `index` of `COMMANDS` acting on `target` at `view`, or
    /// `None` for a command that asks for nothing typed.
    #[must_use]
    pub fn open(index: u16, target: Target, view: &PlayerView) -> Option<Self> {
        let command = COMMANDS.get(usize::from(index))?;
        let label = command.label.trim_end_matches('…').to_owned();
        let ask = |kind, field: &str, start: String, stage, verb: &str| Self {
            command: index,
            target,
            kind,
            field: field.to_owned(),
            start,
            stage,
            verb: verb.to_owned(),
        };
        if command.id == "table.roll" {
            let kind = Kind::Number {
                min: 2,
                max: 1_000_000,
                current: None,
            };
            return Some(ask(kind, "Sides", "6".to_owned(), Stage::Sides, ""));
        }
        Some(match command.asks {
            Asks::Text | Asks::TextAndNumber => {
                let stack_until = command.id == "library.stack_until";
                let field = if stack_until {
                    "Card name or type"
                } else {
                    &label
                };
                let stage = if command.asks == Asks::Text {
                    Stage::Only
                } else {
                    Stage::Text
                };
                let start = note(command, target, view).unwrap_or_default();
                ask(Kind::Text, field, start, stage, &label)
            }
            Asks::Number => number(command, target, view, &label, index),
            Asks::Stats => {
                let current = base_stats(target, view);
                let start = format!("{}/{}", current.0, current.1);
                ask(Kind::Stats { current }, &label, start, Stage::Only, "Set")
            }
            Asks::None | Asks::Target | Asks::Token => return None,
        })
    }

    /// The prompt for the X of the ability at `row` of the permanent `target`, which `source`
    /// names, as the web's `activateAbility` asks it (`ui/src/table/abilities.ts`): any number
    /// from 0, starting at 0, confirmed as "Set X to 3".
    #[must_use]
    pub fn ability_x(source: TableObjectRef, row: usize, target: Target) -> Self {
        Self {
            command: commands::command("card.ability").map_or(u16::MAX, |(index, _)| index),
            target,
            kind: Kind::Number {
                min: 0,
                max: 65_535,
                current: None,
            },
            field: "Choose X".to_owned(),
            start: "0".to_owned(),
            stage: Stage::X { source, row },
            verb: "Set X to".to_owned(),
        }
    }

    /// The field's state when the prompt opens: its starting text, all selected.
    #[must_use]
    pub fn field_state(&self) -> FieldState {
        let end = u32::try_from(self.start.len()).unwrap_or(0);
        FieldState {
            text: self.start.clone(),
            caret: end,
            selection: (end > 0).then_some(0..end),
            placeholder: String::new(),
        }
    }

    /// What `text` reads as, within the prompt's bounds, or `None` if it is no value.
    #[must_use]
    pub fn read(&self, text: &str) -> Option<Entry> {
        match self.kind {
            Kind::Number { min, max, current } => {
                let (value, change) = parse(text, current)?;
                let value = value.clamp(min, max);
                let change = change.and(current.map(|current| value - current));
                Some(Entry::Number { value, change })
            }
            Kind::Text => Some(Entry::Text(text.to_owned())),
            Kind::Stats { current } => {
                let (power, toughness) = parse_stats(text, current)?;
                Some(Entry::Stats(
                    power.clamp(MIN_INT, MAX_INT),
                    toughness.clamp(MIN_INT, MAX_INT),
                ))
            }
        }
    }

    /// What the confirm button says for `text`: what will happen, or "Confirm" while the text
    /// is no value.
    #[must_use]
    pub fn label(&self, text: &str) -> String {
        let Some(entry) = self.read(text) else {
            return "Confirm".to_owned();
        };
        match (&self.stage, entry) {
            (Stage::Sides | Stage::Text, _) => "Next".to_owned(),
            (Stage::Dice { sides }, Entry::Number { value, .. }) => format!("Roll {value}d{sides}"),
            (Stage::Count { text }, Entry::Number { value, .. }) => {
                if self.field == "Hits" {
                    "Put cards on stack".to_owned()
                } else {
                    format!("Add {value} {text}")
                }
            }
            (_, Entry::Stats(power, toughness)) => format!("Set {power}/{toughness}"),
            (_, Entry::Number { value, .. }) => format!("{} {value}", self.verb),
            (_, Entry::Text(_)) => self.verb.clone(),
        }
    }

    /// The text the minus (`by` −1) or plus (+1) beside a number or stats leaves in the field,
    /// or `None` where the prompt has no steps or the field holds no value.
    #[must_use]
    pub fn step(&self, text: &str, by: i64) -> Option<String> {
        match (self.kind, self.read(text)?) {
            (Kind::Number { min, max, .. }, Entry::Number { value, .. }) => {
                Some((value.saturating_add(by)).clamp(min, max).to_string())
            }
            (Kind::Stats { .. }, Entry::Stats(power, toughness)) => {
                let step = |value: i64| value.saturating_add(by).clamp(MIN_INT, MAX_INT);
                Some(format!("{}/{}", step(power), step(toughness)))
            }
            _ => None,
        }
    }

    /// Confirms `text`: the next prompt of a command that asks for two things, or the actions
    /// the command makes with what was typed at `view`. `None` while the text is no value, or
    /// is empty where a text is needed first.
    #[must_use]
    pub fn confirm(&self, text: &str, view: &PlayerView) -> Option<Confirmed> {
        let entry = self.read(text)?;
        if let (Stage::X { source, row }, Entry::Number { value, .. }) = (&self.stage, &entry) {
            return Some(Confirmed::Answer(Answer::ActivateAbility {
                source: source.clone(),
                row: *row,
                x: u16::try_from(*value).ok(),
            }));
        }
        let command = COMMANDS.get(usize::from(self.command))?;
        let mut values = Values::new();
        let next = |kind, field: &str, stage, verb: &str| {
            Confirmed::Next(Box::new(Self {
                kind,
                field: field.to_owned(),
                start: "1".to_owned(),
                stage,
                verb: verb.to_owned(),
                ..self.clone()
            }))
        };
        let mut change = None;
        match (&self.stage, entry) {
            (Stage::Sides, Entry::Number { value, .. }) => {
                let kind = Kind::Number {
                    min: 1,
                    max: 100,
                    current: None,
                };
                return Some(next(kind, "Dice", Stage::Dice { sides: value }, ""));
            }
            (Stage::Text, Entry::Text(text)) => {
                if text.trim().is_empty() {
                    return None;
                }
                let stack_until = command.id == "library.stack_until";
                let kind = Kind::Number {
                    min: 1,
                    max: if stack_until { 65_535 } else { MAX_INT },
                    current: None,
                };
                let field = if stack_until { "Hits" } else { "Number" };
                return Some(next(kind, field, Stage::Count { text }, ""));
            }
            (Stage::Dice { sides }, Entry::Number { value, .. }) => {
                values.insert("number", (*sides).into());
                values.insert("count", value.into());
            }
            (Stage::Count { text }, Entry::Number { value, .. }) => {
                values.insert("text", text.clone().into());
                values.insert("number", value.into());
            }
            (_, Entry::Number { value, change: by }) => {
                values.insert("number", value.into());
                change = by;
            }
            (_, Entry::Text(text)) => {
                let empty_note = command.id == "permanent.note" && text.is_empty();
                values.insert("text", if empty_note { Value::Null } else { text.into() });
            }
            (_, Entry::Stats(power, toughness)) => {
                values.insert("stats", serde_json::json!([power, toughness]));
            }
        }
        if let Some(flow) = look_flow(command, self.target, view, &values) {
            return Some(Confirmed::Send(vec![flow]));
        }
        let actions = commands::actions_with(command, self.target, view, &values);
        Some(Confirmed::Send(match change {
            Some(by) => actions
                .into_iter()
                .map(|action| relative(action, by))
                .collect(),
            None => actions,
        }))
    }
}

/// With Library flows on, Look at top cards on the viewer's own library opens the shared
/// sorting tray instead of only showing the cards: the look becomes a flow of the number asked
/// for (`flowAction` and `usesLookSort` in `ui/src/table/flows.ts`). `None` otherwise.
fn look_flow(
    command: &Command,
    target: Target,
    view: &PlayerView,
    values: &Values,
) -> Option<TableAction> {
    let flows = commands::assist_on(view, commands::Assist::LibraryFlows);
    if command.id != "library.look_top"
        || target != Target::Pile(Side::You, ZoneName::Library)
        || !flows
    {
        return None;
    }
    let count = values.get("number")?.as_u64()?;
    let seat = commands::seat_of(view, Side::You);
    let json =
        serde_json::json!({ "flow": { "seat": seat, "flow": "look_at_top", "count": count } });
    serde_json::from_value(json).ok()
}

/// The prompt for a command that asks for a number, as the web starts and bounds it: a life
/// total starts at the current total and takes any integer; a mulligan starts at the hand's
/// size and may be zero; a library command is bounded by the cards in the library; anything
/// else starts at 1.
fn number(command: &Command, target: Target, view: &PlayerView, label: &str, index: u16) -> Ask {
    let side = target.side().unwrap_or(Side::You);
    let life = matches!(target, Target::Life(_)).then(|| i64::from(player_life(view, side)));
    let hand = (view.table.as_ref()).map_or(7, |table| match side {
        Side::You => table.orders.you.hand.len(),
        Side::Opponent => table.orders.opponent.hand.len(),
    });
    let counter = command.id.starts_with("counter.");
    let library = matches!(target, Target::Pile(_, ZoneName::Library)).then(|| match side {
        Side::You => view.players.you.library.len(),
        Side::Opponent => view.players.opponent.library.len(),
    });
    let mulligan = command.id == "hand.mulligan";
    let start = life.unwrap_or(if mulligan {
        i64::try_from(hand).unwrap_or(7)
    } else {
        1
    });
    let min = if life.is_some() {
        MIN_INT
    } else {
        // A counter or a mulligan may be zero; anything else is at least one.
        i64::from(!(counter || mulligan))
    };
    let max = if life.is_some() {
        MAX_INT
    } else if counter {
        999
    } else {
        let count = library.map_or(65_535, |count| i64::try_from(count).unwrap_or(65_535));
        count.clamp(1, 65_535)
    };
    let verb = match command.id {
        "library.draw_cards" => "Draw",
        "library.move_top_cards_to_graveyard" => "Mill",
        _ => label,
    };
    let field = if command.id == "library.draw_cards" {
        "Cards to draw"
    } else {
        label
    };
    Ask {
        command: index,
        target,
        kind: Kind::Number {
            min,
            max,
            current: life,
        },
        field: field.to_owned(),
        start: start.clamp(min, max).to_string(),
        stage: Stage::Only,
        verb: verb.to_owned(),
    }
}

/// `side`'s life total at `view`.
fn player_life(view: &PlayerView, side: Side) -> i32 {
    match side {
        Side::You => view.players.you.life,
        Side::Opponent => view.players.opponent.life,
    }
}

/// The note a permanent's Note command starts from: the note it has, if any.
fn note(command: &Command, target: Target, view: &PlayerView) -> Option<String> {
    let Target::Card(slot) = target else {
        return None;
    };
    if command.id != "permanent.note" {
        return None;
    }
    let object = view.objects.iter().find(|object| object.slot == slot)?;
    object.note.clone()
}

/// The power and toughness a card's Set power and toughness starts from: its editable stats
/// before counters, as the table sends them, or what it shows, or 0/0 (`baseStats`).
fn base_stats(target: Target, view: &PlayerView) -> (i64, i64) {
    let Target::Card(slot) = target else {
        return (0, 0);
    };
    let base = (view.table.as_ref())
        .and_then(|table| table.card_commands.get(&slot))
        .and_then(|commands| commands.base_stats);
    let shown = view.objects.iter().find(|object| object.slot == slot);
    base.or_else(|| shown.and_then(|object| object.pt))
        .map_or((0, 0), |pt| (i64::from(pt.power), i64::from(pt.toughness)))
}

/// `action` with a value it sets changed into the same change made by `by`: a life total,
/// a player's counter or a card's counter set to the current value plus `by` becomes `by`
/// added, so that "+3" in a life prompt is sent as a gain of three, not as a new total that
/// would overwrite a change the other player made meanwhile.
fn relative(action: TableAction, by: i64) -> TableAction {
    let Ok(Value::Object(mut outer)) = serde_json::to_value(&action) else {
        return action;
    };
    let changed = outer.values_mut().any(|inner| {
        let Value::Object(fields) = inner else {
            return false;
        };
        if fields.remove("set").is_none() {
            return false;
        }
        fields.insert("add".to_owned(), by.into());
        true
    });
    if !changed {
        return action;
    }
    serde_json::from_value(Value::Object(outer)).unwrap_or(action)
}

/// The common counter names Add counter offers above its field, as the web's
/// `counterSuggestions` (`ui/src/table/cardState.ts`): a press on one adds that counter.
pub const COUNTER_SUGGESTIONS: [&str; 6] = [
    "+1/+1",
    "\u{2212}1/\u{2212}1",
    "loyalty",
    "charge",
    "lore",
    "stun",
];

/// The common counter names of the ask prompt open for `ask`, if its command offers them: Add
/// counter's, as the web's `TableMenu` gives the prompt `suggestions` for a named counter.
#[must_use]
pub fn suggestions(ask: &Ask) -> Option<&'static [&'static str]> {
    let command = COMMANDS.get(usize::from(ask.command))?;
    (command.id == "counter.add_named").then_some(&COUNTER_SUGGESTIONS[..])
}

/// Places the phone's ask prompt before its element is built or reused.
#[must_use]
pub fn dock_anchor(
    ask: &Ask,
    profile: Profile<'_>,
    layout: Option<&PlacementScene>,
) -> Option<Anchor> {
    let (bands, metrics) = profile.bands().zip(profile.data().table.banded.as_ref())?;
    Some(super::profiles::ask_anchor(
        bands,
        metrics,
        suggestions(ask).is_some(),
        layout,
    ))
}

/// The ask prompt in the dock at the lower right of a window of `window`'s size: Cancel, Add
/// counter's common counter names, the field with a minus and a plus beside a number or stats,
/// and the confirm button.
#[must_use]
pub fn dock(
    ask: &Ask,
    fields: &Fields,
    window: Window,
    profile: Profile<'_>,
    anchor: Option<Anchor>,
    inset: f32,
) -> Element {
    let state = fields.get(&ask.field);
    let text = &state.text;
    let value = ask.read(text);
    let step = |by: i64, sign: &str, name: &str| {
        ask.step(text, by).map(|next| {
            button(sign)
                .name(name)
                .enabled(value.is_some() && next != *text)
                .on_press(AskMessage::Step(by.is_positive()))
        })
    };
    let input = field(&ask.field, &state);
    let confirm = primary(&ask.label(text))
        .name("confirm")
        .enabled(value.is_some())
        .on_press(AskMessage::Confirm);
    let cancel = button("Cancel").name("close").on_press(AskMessage::Close);
    let minus = step(-1, "−", "Less");
    let plus = step(1, "+", "More");
    let panel = column()
        .name("ask")
        .background(color::PANEL)
        .border((color::METAL, size::BORDER))
        .radius(size::PANEL_RADIUS);
    if let Some(banded) = profile.bands().zip(profile.data().table.banded.as_ref()) {
        return super::profiles::ask_dock(
            panel,
            [minus, plus],
            input,
            [cancel, confirm],
            suggestions(ask),
            (banded.0, banded.1, anchor),
        );
    }
    let controls = row()
        .gap(space::M)
        .align_items(Align::Center)
        .child(cancel)
        .children(minus)
        .child(input.min_width(size::BUTTON_MIN_WIDTH))
        .children(plus)
        .child(confirm);
    // The common counter names stand in a row of small buttons above the field, as the web's
    // `.table-ask-suggestions` does.
    let common = suggestions(ask).map(|names| {
        row()
            .name("Common counters")
            .gap(space::S)
            .children(names.iter().enumerate().map(|(at, name)| {
                let at = u8::try_from(at).unwrap_or(u8::MAX);
                button(name)
                    .min_width(0.0)
                    .on_press(AskMessage::Suggestion(at))
            }))
    });
    let mut dock = panel
        .gap(space::M)
        .padding(space::L)
        .align_items(Align::End)
        .children(common)
        .child(controls);
    let corner = Vec2::new(window.size.width - inset, window.size.height - inset);
    dock.anchor = Some(Anchor::Point(corner, Vec2::ONE));
    dock
}
