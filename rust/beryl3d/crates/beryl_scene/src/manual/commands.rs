//! The manual table's commands (docs/manual-multiplayer.md, "Commands"): every command of
//! `data/table-commands.json`, read at build time into `COMMANDS`, which commands apply to a
//! target (`commands_for`, the web's `commandsFor` in `ui/src/table/commands.ts`), and the
//! table actions a command makes for a target (`actions`, the web's `commandActions` in
//! `ui/src/table/commandActions.ts`).

use std::collections::BTreeMap;

use beryl_protocol::PlayerView;
use beryl_protocol::protocol::{
    FaceName, ManualPiles, ObjectId, ObjectView, PlayerState, Side, TableAction, TableObjectAction,
    TableView, ZoneName,
};
use beryl_protocol::script::Subject;
use serde_json::Value;

use super::{Context, Target};

/// What a command applies to: the kind of thing a menu is opened on or a key acts on
/// (`AppliesTo` in commands.ts).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppliesTo {
    Permanent,
    TheirPermanent,
    HandCard,
    StackEntry,
    PileCard,
    Library,
    Graveyard,
    Exile,
    Sideboard,
    Hand,
    Life,
    Player,
    Table,
    Lobby,
    ViewerCard,
}

/// Where a command is given: at the table or in its lobby.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Table,
    Lobby,
}

/// What a command asks the player for before it acts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asks {
    None,
    Number,
    Text,
    TextAndNumber,
    Stats,
    Target,
    Token,
}

/// The table assist a command belongs to, which offers it only while that assist is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Assist {
    TurnUpkeep,
    Combat,
    StateNudges,
    LibraryFlows,
    AbilityMenus,
}

/// A command's key, as the web writes a binding, on the Mac and elsewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandKey {
    pub mac: &'static str,
    pub other: &'static str,
}

/// Where a command's row stands in a context menu: its group, its order among all the menu's
/// rows, and the submenu it is folded into, if any.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub group: &'static str,
    pub order: u32,
    pub submenu: Option<&'static str>,
}

/// One command of `data/table-commands.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Command {
    pub id: &'static str,
    pub label: &'static str,
    pub applies_to: AppliesTo,
    pub scope: Scope,
    pub asks: Asks,
    pub key: Option<CommandKey>,
    pub key_aliases: &'static [CommandKey],
    pub menu: Option<Placement>,
    /// The other targets the command applies to, each with its menu placement there, or none
    /// when it applies there with no row in the menu.
    pub also_applies_to: &'static [(AppliesTo, Option<Placement>)],
    pub group: &'static str,
    pub assist: Option<Assist>,
    /// The condition of the table or target under which the command is offered, if any.
    pub when: Option<&'static str>,
    /// The table action the command makes, as JSON whose `"$name"` strings are filled in.
    pub action: Option<&'static str>,
    /// What the client does for a command that makes no table action, such as `leave_table`.
    pub client: Option<&'static str>,
}

include!(concat!(env!("OUT_DIR"), "/table_commands.rs"));

impl Command {
    /// Whether the command applies to targets of the kind `kind`, with a menu row there or not
    /// (`applies` in `ui/src/table/TableMenu.tsx`).
    #[must_use]
    pub fn applies(&self, kind: AppliesTo) -> bool {
        self.applies_to == kind || self.also_applies_to.iter().any(|(other, _)| *other == kind)
    }
}

/// A command that applies to a target, with its menu placement there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub command: &'static Command,
    /// The command's index in `COMMANDS`.
    pub index: u16,
    pub menu: Option<Placement>,
}

/// The commands of the table's scope that apply to `applies_to`, in the file's order, with
/// their menu placement there, as the web's `commandsFor` gives them: a command of an assist
/// only while `view` has that assist on, and a command with a condition only while the
/// condition holds (`conditions`) at `view` and in what the client knows (`context`).
#[must_use]
pub fn commands_for(applies_to: AppliesTo, view: &PlayerView, context: &Context) -> Vec<Entry> {
    let mut entries = Vec::new();
    for (index, command) in COMMANDS.iter().enumerate() {
        if command.scope != Scope::Table
            || command
                .assist
                .is_some_and(|assist| !assist_on(view, assist))
            || command
                .when
                .is_some_and(|when| !condition(view, context, when))
        {
            continue;
        }
        let menu = if command.applies_to == applies_to {
            command.menu
        } else if let Some((_, placement)) =
            (command.also_applies_to.iter()).find(|(target, _)| *target == applies_to)
        {
            *placement
        } else {
            continue;
        };
        entries.push(Entry {
            command,
            index: u16::try_from(index).expect("fewer than 65,536 commands"),
            menu,
        });
    }
    entries
}

/// The commands offered for one card by what its visible face prints, which
/// `commands_for` leaves out because their condition is about the card, not the table: Activate
/// ability and the card's library instructions, each while its assist is on, with its menu
/// placement for targets of the kind `applies_to`. The context menu makes their rows from the
/// card (`menus::card_rows`).
#[must_use]
pub fn card_entries(applies_to: AppliesTo, view: &PlayerView) -> Vec<Entry> {
    const CARD_CONDITIONS: [&str; 4] = [
        "activated_abilities",
        "card_scry",
        "card_surveil",
        "card_look_at_top",
    ];
    let mut entries = Vec::new();
    for (index, command) in COMMANDS.iter().enumerate() {
        if !command
            .when
            .is_some_and(|when| CARD_CONDITIONS.contains(&when))
            || command
                .assist
                .is_some_and(|assist| !assist_on(view, assist))
        {
            continue;
        }
        let menu = if command.applies_to == applies_to {
            command.menu
        } else if let Some((_, placement)) =
            (command.also_applies_to.iter()).find(|(target, _)| *target == applies_to)
        {
            *placement
        } else {
            continue;
        };
        entries.push(Entry {
            command,
            index: u16::try_from(index).expect("fewer than 65,536 commands"),
            menu,
        });
    }
    entries
}

/// The command whose id is `id`, if any.
#[must_use]
pub fn command(id: &str) -> Option<(u16, &'static Command)> {
    let index = COMMANDS.iter().position(|command| command.id == id)?;
    Some((u16::try_from(index).ok()?, &COMMANDS[index]))
}

/// The index in `COMMANDS` of `command`, one of its entries.
#[must_use]
pub fn index_of(command: &Command) -> u16 {
    let index = (COMMANDS.iter()).position(|entry| entry.id == command.id);
    index
        .and_then(|index| u16::try_from(index).ok())
        .unwrap_or(u16::MAX)
}

/// Whether `view`'s table has `assist` on.
pub(crate) fn assist_on(view: &PlayerView, assist: Assist) -> bool {
    let Some(table) = &view.table else {
        return false;
    };
    let assists = &table.assists;
    match assist {
        Assist::TurnUpkeep => assists.turn_upkeep,
        Assist::Combat => assists.combat,
        Assist::StateNudges => assists.state_nudges,
        Assist::LibraryFlows => assists.library_flows,
        Assist::AbilityMenus => assists.ability_menus,
    }
}

/// Whether the condition `when` holds at `view`'s table, with what the client knows
/// (`context`). Only `last_draw`, `last_token` and the deck tokens are read here: the
/// conditions on a selection, a viewer and flows hold once the client has those, and until
/// then their commands are not offered, as the web offers none whose condition fails.
fn condition(view: &PlayerView, context: &Context, when: &str) -> bool {
    let Some(table) = &view.table else {
        return false;
    };
    match when {
        "last_draw" => table.last_draw.is_some(),
        "last_token" => context.last_token.is_some(),
        _ => deck_token(when, "deck_token_").is_some_and(|n| n < table.deck_tokens.len()),
    }
}

/// The index into the table's deck tokens that `id` names after `prefix`, counting from 1 as
/// the numbered shortcuts do: `token.deck_3` names the third. `None` for any other id.
fn deck_token(id: &str, prefix: &str) -> Option<usize> {
    let number: usize = id.strip_prefix(prefix)?.parse().ok()?;
    number.checked_sub(1)
}

/// The action a token command makes: its template with the token filled in, which is the
/// `card` in `values` (the token search's choice) or, for a numbered deck token's command, that
/// token of the table's deck tokens. Empty when neither gives one. A token made with Shift
/// held is made face down (`values["face_down"]`), as the web's `commandActions` makes it.
fn token_actions(
    command: &Command,
    template: &Value,
    view: &PlayerView,
    values: &Values,
) -> Vec<TableAction> {
    let deck = deck_token(command.id, "token.deck_")
        .and_then(|index| view.table.as_ref()?.deck_tokens.get(index))
        .map(|token| {
            serde_json::json!({ "token": { "name": token.name, "scryfall_id": token.scryfall_id } })
        });
    let Some(card) = values.get("card").cloned().or(deck) else {
        return Vec::new();
    };
    let mut args: BTreeMap<&str, Value> = BTreeMap::new();
    args.insert("card", card);
    args.insert("text", values.get("text").cloned().unwrap_or(Value::Null));
    let Some(Value::Object(mut action)) = substitute(template, &args) else {
        return Vec::new();
    };
    if values.get("face_down") == Some(&Value::Bool(true))
        && let Some(Value::Object(token)) = action.get_mut("token")
    {
        token.insert("face_down".to_owned(), Value::Bool(true));
    }
    serde_json::from_value(Value::Object(action))
        .ok()
        .into_iter()
        .collect()
}

/// The seat of `side` at `view`'s table: the viewer's own seat for `Side::You`, and the other
/// one for the opponent (`seatOf` in `ui/src/table/menuRows.ts`).
#[must_use]
pub fn seat_of(view: &PlayerView, side: Side) -> u8 {
    let you = view
        .table
        .as_ref()
        .and_then(|table| table.seat)
        .unwrap_or(0);
    match side {
        Side::You => you,
        Side::Opponent => 1 - you.min(1),
    }
}

/// Whether the command's row shows a check mark for these cards, or `None` for a command
/// that has none (`checked` in menuRows.ts).
fn checked(command: &Command, view: &PlayerView, target: Target, cards: &[Card]) -> Option<bool> {
    let table = view.table.as_ref()?;
    let assist = |name: &str| match name {
        "turn_upkeep" => Some(Assist::TurnUpkeep),
        "combat" => Some(Assist::Combat),
        "state_nudges" => Some(Assist::StateNudges),
        "library_flows" => Some(Assist::LibraryFlows),
        "ability_menus" => Some(Assist::AbilityMenus),
        _ => None,
    };
    match command.id {
        "permanent.skip_untap" => Some(
            !cards.is_empty()
                && cards
                    .iter()
                    .all(|card| card.object.is_some_and(|object| object.doesnt_untap)),
        ),
        "library.keep_top_revealed" if matches!(target, Target::Pile(_, ZoneName::Library)) => {
            Some(table.top_revealed)
        }
        "library.keep_top_known" => Some(table.top_known),
        id => {
            let name = id.strip_prefix("assist.")?;
            Some(assist(name).is_some_and(|assist| assist_on(view, assist)))
        }
    }
}

/// A card a command acts on: what the view shows of it and the reference an action names it
/// by.
#[derive(Clone, Copy)]
struct Card<'a> {
    object: Option<&'a ObjectView>,
    action: &'a TableObjectAction,
}

/// The object `slot` of `view` and the reference its table gives it, if it has one.
fn card(view: &PlayerView, slot: ObjectId) -> Option<Card<'_>> {
    let table = view.table.as_ref()?;
    let action =
        (table.object_actions.iter()).find(|action| action.subject == Subject::Object { slot })?;
    let object = view.objects.iter().find(|object| object.slot == slot);
    Some(Card { object, action })
}

/// The power and toughness each stat command adds (`statChanges` in commandActions.ts).
const STAT_CHANGES: [&str; 8] = [
    "stats.power_up",
    "stats.power_down",
    "stats.toughness_up",
    "stats.toughness_down",
    "stats.both_up",
    "stats.both_down",
    "stats.power_up_toughness_down",
    "stats.power_down_toughness_up",
];

/// The table actions `command` makes for `target` at `view`, as the web's `commandActions`
/// makes them for a command that asks for nothing: its template with the seat, the cards and
/// their zones filled in, the cards grouped by zone, owner and face so that each group moves
/// as one action. Empty when the command makes no table action here: a client command, one
/// that asks the player for a value first, a stat change or token (which need the card's stats
/// or the token chosen), or one whose cards have no reference in this view.
#[must_use]
pub fn actions(command: &Command, target: Target, view: &PlayerView) -> Vec<TableAction> {
    actions_with(command, target, view, &Values::new())
}

/// The values a command's template is filled with beyond those `actions_with` finds itself:
/// what the player gave the ask prompt (`number`, `text`, `stats`, `count`) or the token
/// search (`card`), each a JSON value named as the template names it.
pub type Values = BTreeMap<&'static str, Value>;

/// The table actions `command` makes for `target` at `view`, as `actions` makes them, with
/// `values` filled in over its own (`commandActions` with its `values` argument). A number
/// given to a library command is how many cards it takes from the end it names; a `card` given
/// to a token command is the token it makes.
#[must_use]
pub fn actions_with(
    command: &Command,
    target: Target,
    view: &PlayerView,
    values: &Values,
) -> Vec<TableAction> {
    actions_for(command, target, &[], view, values)
}

/// The table actions `command` makes for `target` at `view`, as `actions_with` makes them,
/// except that a card target among the `selected` cards stands for all of them, as the web's
/// commands act on the whole selection when the card they are given is in it.
#[must_use]
pub fn actions_for(
    command: &Command,
    target: Target,
    selected: &[ObjectId],
    view: &PlayerView,
    values: &Values,
) -> Vec<TableAction> {
    if matches!(command.id, "table.untap_all" | "phase.untap_active") {
        return vec![untap(view)];
    }
    let Some(template) = command.action else {
        return Vec::new();
    };
    let Ok(template) = serde_json::from_str::<Value>(template) else {
        return Vec::new();
    };
    if template.get("token").is_some() {
        return token_actions(command, &template, view, values);
    }
    if STAT_CHANGES.contains(&command.id) {
        return Vec::new();
    }
    let Some(table) = &view.table else {
        return Vec::new();
    };
    let side = target.side().unwrap_or(Side::You);
    let seat = seat_of(view, side);
    let mut cards: Vec<Card> = match target {
        Target::Card(slot) if selected.contains(&slot) => selected
            .iter()
            .filter_map(|&slot| card(view, slot))
            .collect(),
        Target::Card(slot) => card(view, slot).into_iter().collect(),
        _ => Vec::new(),
    };
    let mut args: BTreeMap<&str, Value> = BTreeMap::new();
    args.insert("seat", seat.into());
    args.insert("other_seat", (1 - seat_of(view, Side::You).min(1)).into());
    args.insert("owner", seat.into());
    if let Some(entry) = table.last_draw {
        args.insert("entry", entry.into());
    }
    args.insert("count", 1.into());
    args.insert("text", Value::Null);
    let on = !checked(command, view, target, &cards).unwrap_or(false);
    args.insert("on", on.into());
    let tapped = (cards.iter()).any(|card| card.object.is_some_and(|object| !object.tapped));
    args.insert("tapped", tapped.into());
    for (name, value) in values {
        args.insert(name, value.clone());
    }
    let hand = piles(table, side).hand.len();
    match command.id {
        "hand.mulligan_same_size" => {
            args.insert("number", hand.into());
        }
        "hand.mulligan_one_fewer" => {
            args.insert("number", hand.saturating_sub(1).into());
        }
        _ => {}
    }
    let pile = match target {
        Target::Pile(_, zone) => Some(zone),
        Target::Hand(_) => Some(ZoneName::Hand),
        _ => None,
    };
    if !uses(&template, "objects") {
        if let Some(zone) = pile {
            args.insert("in", serde_json::json!({ zone_word(zone): seat }));
        }
        return fill(&template, &args).into_iter().collect();
    }
    if let Some(zone) = pile {
        let count = (values.get("number").and_then(Value::as_u64))
            .and_then(|count| usize::try_from(count).ok())
            .unwrap_or(1);
        let Some(found) = pile_cards(command, zone, side, count, view, table) else {
            return Vec::new();
        };
        cards = found;
    }
    if COUNTERS.iter().any(|name| uses(&template, name)) {
        return counter_actions(&template, &args, &cards, table);
    }
    grouped_actions(command, &template, &args, &cards, view, seat)
}

/// The names a template gives the first three kinds of counter a card has.
const COUNTERS: [&str; 3] = ["first_counter", "second_counter", "third_counter"];

/// The cards a pile command takes from `side`'s pile `zone`: a library command `count` cards
/// from the end it names, and every other pile command the whole pile, the top card first.
/// `None` when one of them has no reference in this view.
fn pile_cards<'a>(
    command: &Command,
    zone: ZoneName,
    side: Side,
    count: usize,
    view: &'a PlayerView,
    table: &'a TableView,
) -> Option<Vec<Card<'a>>> {
    let order: &[ObjectId] = match zone {
        ZoneName::Library => &player(view, side).library,
        ZoneName::Graveyard => &player(view, side).graveyard,
        ZoneName::Hand => &piles(table, side).hand,
        ZoneName::Exile => &piles(table, side).exile,
        _ => &[],
    };
    let selected: Vec<ObjectId> = match zone {
        ZoneName::Library if command.id.contains("bottom") => {
            order.iter().take(count).copied().collect()
        }
        ZoneName::Library => order.iter().rev().take(count).copied().collect(),
        ZoneName::Hand => order.to_vec(),
        _ => order.iter().rev().copied().collect(),
    };
    selected.iter().map(|&slot| card(view, slot)).collect()
}

/// One action for each card of a template that names a card's counters by their place, with
/// that card's counter names filled in.
fn counter_actions(
    template: &Value,
    args: &BTreeMap<&str, Value>,
    cards: &[Card],
    table: &TableView,
) -> Vec<TableAction> {
    (cards.iter())
        .filter_map(|card| {
            let names = card
                .object
                .and_then(|object| table.card_commands.get(&object.slot))
                .map(|commands| commands.counter_names.clone())?;
            let mut args = args.clone();
            args.insert("objects", serde_json::json!([card.action.reference]));
            args.insert("in", serde_json::to_value(card.action.zone).ok()?);
            for (name, counter) in COUNTERS.into_iter().zip(names) {
                args.insert(name, counter.into());
            }
            fill(template, &args)
        })
        .collect()
}

/// The actions of a template that names cards: the cards grouped by zone, and by owner and
/// face where the template names them, each group one action.
fn grouped_actions(
    command: &Command,
    template: &Value,
    args: &BTreeMap<&str, Value>,
    cards: &[Card],
    view: &PlayerView,
    seat: u8,
) -> Vec<TableAction> {
    let mut groups: Vec<(Value, u8, &str, Vec<Value>)> = Vec::new();
    for card in cards {
        let Ok(zone) = serde_json::to_value(card.action.zone) else {
            continue;
        };
        let owner = card
            .object
            .map_or(seat, |object| seat_of(view, object.owner));
        let face = card.object.map_or(FaceName::Front, |object| object.face);
        let face = if command.id == "card.transform" {
            if face == FaceName::Back {
                "front"
            } else {
                "back"
            }
        } else if face == FaceName::Down {
            "front"
        } else {
            "down"
        };
        let reference = serde_json::to_value(&card.action.reference).unwrap_or(Value::Null);
        match groups.last_mut() {
            Some((last_zone, last_owner, last_face, references))
                if *last_zone == zone
                    && (!uses(template, "owner") || *last_owner == owner)
                    && (!uses(template, "face") || *last_face == face) =>
            {
                references.push(reference);
            }
            _ => groups.push((zone, owner, face, vec![reference])),
        }
    }
    // Each group moves as one action, and putting groups on top of a library one after
    // another reverses their order, so they are given last first.
    if matches!(command.id, "move.library_top" | "move.library_nth") {
        groups.reverse();
    }
    groups
        .into_iter()
        .filter_map(|(zone, owner, face, references)| {
            let mut args = args.clone();
            args.insert("objects", references.into());
            args.insert("in", zone);
            args.insert("owner", owner.into());
            args.insert("face", face.into());
            fill(template, &args)
        })
        .collect()
}

/// The action that untaps every permanent the viewer controls that is tapped and untaps as
/// usual (`untapPermanents` in `ui/src/table/turn.ts`).
fn untap(view: &PlayerView) -> TableAction {
    let tapped: Vec<ObjectId> = (view.objects.iter())
        .filter(|object| {
            object.zone == ZoneName::Battlefield
                && object.controller == Side::You
                && object.tapped
                && !object.doesnt_untap
        })
        .map(|object| object.slot)
        .collect();
    let objects: Vec<Value> = (view.table.iter())
        .flat_map(|table| &table.object_actions)
        .filter(|action| match action.subject {
            Subject::Object { slot } => tapped.contains(&slot),
            _ => false,
        })
        .filter_map(|action| serde_json::to_value(&action.reference).ok())
        .collect();
    let action = serde_json::json!({
        "tap": { "objects": objects, "in": "battlefield", "tapped": false }
    });
    serde_json::from_value(action).expect("an untap action is a table action")
}

/// `side`'s public state in `view`.
fn player(view: &PlayerView, side: Side) -> &PlayerState {
    match side {
        Side::You => &view.players.you,
        Side::Opponent => &view.players.opponent,
    }
}

/// The order of `side`'s hand, exile and sideboard at `table`.
fn piles(table: &TableView, side: Side) -> &ManualPiles {
    match side {
        Side::You => &table.orders.you,
        Side::Opponent => &table.orders.opponent,
    }
}

/// The word a table action names the zone `zone` by.
fn zone_word(zone: ZoneName) -> &'static str {
    match zone {
        ZoneName::Library => "library",
        ZoneName::Graveyard => "graveyard",
        ZoneName::Exile => "exile",
        _ => "hand",
    }
}

/// Whether `template` names the value `name` anywhere (`uses` in `ui/src/table/fill.ts`).
fn uses(template: &Value, name: &str) -> bool {
    match template {
        Value::String(text) => text.strip_prefix('$') == Some(name),
        Value::Array(items) => items.iter().any(|item| uses(item, name)),
        Value::Object(fields) => fields.values().any(|value| uses(value, name)),
        _ => false,
    }
}

/// `template` with each `"$name"` string replaced by the value of `name` in `values`, read as
/// a table action; `None` when it names a value `values` lacks or is no table action
/// (`fill` in fill.ts).
fn fill(template: &Value, values: &BTreeMap<&str, Value>) -> Option<TableAction> {
    serde_json::from_value(substitute(template, values)?).ok()
}

fn substitute(template: &Value, values: &BTreeMap<&str, Value>) -> Option<Value> {
    Some(match template {
        Value::String(text) => match text.strip_prefix('$') {
            Some(name) => values.get(name)?.clone(),
            None => template.clone(),
        },
        Value::Array(items) => Value::Array(
            (items.iter())
                .map(|item| substitute(item, values))
                .collect::<Option<_>>()?,
        ),
        Value::Object(fields) => Value::Object(
            (fields.iter())
                .map(|(key, value)| Some((key.clone(), substitute(value, values)?)))
                .collect::<Option<_>>()?,
        ),
        _ => template.clone(),
    })
}
