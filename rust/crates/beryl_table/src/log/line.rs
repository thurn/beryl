use std::cell::RefCell;

use serde::{Deserialize, Serialize};

use crate::action::{
    Adjustment, ComputedDestination, Destination, Portion, Position, Selection, Target,
};
use crate::state::{Face, ObjectId, Phase, Zone};
use crate::{Action, Change, Entry, Seat, Table};

/// Connection events come from the adapter and do not mutate the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionEvent {
    Joined,
    Left,
    Lost,
    Restored,
}

pub fn connection_line(event: ConnectionEvent) -> &'static str {
    match event {
        ConnectionEvent::Joined => "Joined the game.",
        ConnectionEvent::Left => "Left the game.",
        ConnectionEvent::Lost => "Lost connection.",
        ConnectionEvent::Restored => "Restored connection.",
    }
}

/// Public facts that must survive redaction and delayed rendering.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Summary {
    count: usize,
    sideboard_size: u32,
    turn_seat: Option<Seat>,
    cast: bool,
    dice: Vec<u32>,
    life: Option<i32>,
    untapped_all: bool,
}

impl Summary {
    pub(crate) fn new(table: &Table, by: Seat, action: &Action, changes: &[Change]) -> Self {
        let mut summary = Self::default();
        match action {
            Action::Draw { seat, .. } | Action::Mulligan { seat, .. } => {
                summary.count = changes
                    .iter()
                    .filter(|change| {
                        matches!(change,
                            Change::Move { from, to, .. }
                            if from.zone == Zone::Library(*seat) && to.zone == Zone::Hand(*seat)
                        )
                    })
                    .count();
            }
            Action::LoadDeck { seat, .. } => {
                summary.sideboard_size = table.game_match.seats[usize::from(seat.0)]
                    .sideboard
                    .iter()
                    .map(|(_, copies)| u32::from(*copies))
                    .sum();
            }
            Action::StackUntil { .. } => {
                summary.count = changes.iter().filter(|change|
                    matches!(change, Change::Move { to, .. } if to.zone == Zone::Stack)
                ).count();
            }
            Action::NextTurn {} => summary.turn_seat = Some(table.active),
            Action::Move {
                to: Destination::Computed(ComputedDestination::Play),
                ..
            } => {
                summary.cast = changes.iter().any(|change| {
                    matches!(change,
                        Change::Move { to, .. } if to.zone == Zone::Stack
                    )
                });
            }
            Action::Roll { .. } => {
                summary.dice = super::outcomes(changes).collect();
            }
            Action::Batch { actions } if paired_rolls(actions) => {
                summary.dice = super::outcomes(changes).collect();
            }
            Action::Batch { actions } => {
                if let [action @ Action::Mulligan { .. }] = actions.as_slice() {
                    return Self::new(table, by, action, changes);
                }
            }
            Action::Life { seat, .. } => {
                summary.life = Some(table.players[usize::from(seat.0)].life);
            }
            Action::Tap {
                selection,
                tapped: false,
            } if selection.objects.len() > 1 => {
                summary.untapped_all = selection
                    .objects
                    .iter()
                    .all(|id| table.objects[id].controller == by)
                    && table.objects.values().all(|object| {
                        object.zone != Zone::Battlefield
                            || object.controller != by
                            || object.doesnt_untap
                            || !object.tapped
                    });
            }
            _ => {}
        }
        summary
    }
}

/// Formats a projected entry against a table projected for the same seat.
/// The caller supplies the actor's name and the log's strike status.
pub fn line(entry: &Entry, table: &Table, for_seat: Seat) -> String {
    let cards = Cards {
        before: table,
        after: table,
        viewer: for_seat,
        recorded_names: None,
        anonymous: false,
    };
    let text = if let Some(action) = &entry.undone_action {
        let description = describe(action, &entry.summary, &cards);
        format!("Undid: {description}")
    } else {
        describe(&entry.action, &entry.summary, &cards)
    };
    finish(text, entry.seen)
}

fn finish(mut text: String, seen: bool) -> String {
    if seen {
        text.push_str(" (seen)");
    }
    text.push('.');
    text
}

/// Captured wording, with an alternate for when a card named here is hidden
/// in a later view.
#[derive(Clone, Debug)]
pub(super) struct RecordedLine {
    pub(super) text: String,
    pub(super) anonymous: String,
    pub(super) names: Vec<String>,
}

impl RecordedLine {
    pub(super) fn undo(&self, seen: bool) -> Self {
        let undo_line = |text: &str| {
            let description = text
                .strip_suffix('.')
                .expect("recorded line ends with a period");
            finish(format!("Undid: {description}"), seen)
        };
        Self {
            text: undo_line(&self.text),
            anonymous: undo_line(&self.anonymous),
            names: self.names.clone(),
        }
    }
}

/// Capture wording against the tables on either side of the action. An
/// action can remove a public object or replace its id before it is logged.
pub(super) fn recorded_line(
    entry: &Entry,
    before: &Table,
    after: &Table,
    viewer: Seat,
) -> RecordedLine {
    let names = RefCell::new(Vec::new());
    let cards = Cards {
        before,
        after,
        viewer,
        recorded_names: Some(&names),
        anonymous: false,
    };
    let text = finish(describe(&entry.action, &entry.summary, &cards), entry.seen);
    let names = names.into_inner();
    let anonymous = if names.is_empty() {
        text.clone()
    } else {
        let cards = Cards {
            before,
            after,
            viewer,
            recorded_names: None,
            anonymous: true,
        };
        finish(describe(&entry.action, &entry.summary, &cards), entry.seen)
    };
    RecordedLine {
        text,
        anonymous,
        names,
    }
}

fn describe(action: &Action, summary: &Summary, names: &Cards<'_>) -> String {
    let viewer = names.viewer;
    let cards = |selection: &Selection| names.selected(selection);
    let pile = |zone| zone_name(zone, viewer);
    let player = |seat| player_name(seat, viewer);
    match action {
        Action::Move {
            selection,
            to,
            position,
            face,
            ..
        } => movement(selection, *to, *position, *face, summary, names),
        Action::Reorder { selection, .. } => {
            format!("Reordered {} in {}", cards(selection), pile(selection.zone))
        }
        Action::Pin { selection, .. } => format!("Changed the row for {}", cards(selection)),
        Action::Give { selection, to } => {
            format!("Gave control of {} to {}", cards(selection), player(*to))
        }
        Action::Tap { tapped: false, .. } if summary.untapped_all => {
            "Untapped all permanents".into()
        }
        Action::Tap { selection, tapped } => {
            let verb = if *tapped { "Tapped" } else { "Untapped" };
            format!("{verb} {}", cards(selection))
        }
        Action::Face { selection, face } => {
            let direction = if *face == Face::Down { "down" } else { "up" };
            format!("Turned {} face {direction}", cards(selection))
        }
        Action::Flag {
            selection,
            doesnt_untap,
        } => {
            let verb = if *doesnt_untap {
                "to skip untap"
            } else {
                "to untap"
            };
            format!("Set {} {verb}", cards(selection))
        }
        Action::Peek { .. } => "Peeked at face-down cards".into(),
        Action::Counter {
            selection,
            name,
            adjustment,
        } => {
            let name = names.counter_label(selection, name);
            format!(
                "{} on {}",
                counter_change(name, *adjustment),
                cards(selection)
            )
        }
        Action::Stats {
            selection,
            stats: Some((power, toughness)),
        } => {
            format!("Set {} to {power}/{toughness}", cards(selection))
        }
        Action::Stats {
            selection,
            stats: None,
        } => format!("Cleared power and toughness for {}", cards(selection)),
        Action::Damage { selection, damage } => {
            format!("Set damage on {} to {damage}", cards(selection))
        }
        Action::Note { selection, .. } => format!("Annotated {}", cards(selection)),
        Action::Attach { selection, host } => {
            format!("Attached {} to {}", cards(selection), names.object(host.id))
        }
        Action::Unattach { selection } => format!("Detached {}", cards(selection)),
        Action::Clone { selection } => format!("Created a token copy of {}", cards(selection)),
        Action::Token { count, .. } => {
            format!("Created {}", amount(usize::from(*count), "token", "tokens"))
        }
        Action::Marker { .. } => "Put an ability on the stack".into(),
        Action::Remove { selection } => format!("Removed {}", cards(selection)),
        Action::Draw { .. }
        | Action::StackUntil { .. }
        | Action::Mulligan { .. }
        | Action::Shuffle { .. }
        | Action::Look { .. }
        | Action::Reveal { .. }
        | Action::Lend { .. }
        | Action::AlwaysRevealTop { .. }
        | Action::AlwaysLookAtTop { .. }
        | Action::SortHand { .. } => library(action, summary, viewer),
        _ => game(action, summary, names),
    }
}

fn library(action: &Action, summary: &Summary, viewer: Seat) -> String {
    let pile = |zone| zone_name(zone, viewer);
    let player = |seat| player_name(seat, viewer);
    match action {
        Action::Draw { .. } if summary.count == 0 => "Drew from an empty library".into(),
        Action::Draw { .. } => format!("Drew {}", card_count(summary.count)),
        Action::StackUntil { .. } => format!("Put {} on the stack", card_count(summary.count)),
        Action::Mulligan { .. } => {
            format!("Mulliganed and drew {}", card_count(summary.count))
        }
        Action::Shuffle { seat, portion } => {
            format!("Shuffled {}", part(Zone::Library(*seat), *portion, viewer))
        }
        Action::Look { seat, portion } => {
            format!("Looked at {}", part(Zone::Library(*seat), *portion, viewer))
        }
        Action::Reveal {
            zone,
            objects,
            random,
            to,
        } => {
            let revealed = match (objects, random) {
                (_, Some(count)) => format!(
                    "{} at random from {}",
                    card_count(usize::from(*count)),
                    pile(*zone)
                ),
                (Some(_), None) => format!("cards from {}", pile(*zone)),
                (None, None) => pile(*zone),
            };
            let recipients: Vec<_> = to.iter().map(|&seat| player(seat)).collect();
            format!("Revealed {revealed} to {}", recipients.join(" and "))
        }
        Action::Lend { zone, to } => format!("Lent {} to {}", pile(*zone), player(*to)),
        Action::AlwaysRevealTop { seat, on } => {
            let verb = if *on {
                "Kept revealed"
            } else {
                "Stopped revealing"
            };
            format!("{verb} the top card of {}", pile(Zone::Library(*seat)))
        }
        Action::AlwaysLookAtTop { seat, on } => {
            let verb = if *on {
                "Started looking at"
            } else {
                "Stopped looking at"
            };
            format!("{verb} the top card of {}", pile(Zone::Library(*seat)))
        }
        Action::SortHand { seat, .. } => format!("Sorted {}", pile(Zone::Hand(*seat))),
        _ => unreachable!("expected a library action"),
    }
}

fn game(action: &Action, summary: &Summary, names: &Cards<'_>) -> String {
    let viewer = names.viewer;
    let player = |seat| player_name(seat, viewer);
    match action {
        Action::Life { seat, .. } => summary.life.map_or_else(
            || "Set life".into(),
            |life| format!("Set life for {} to {life}", player(*seat)),
        ),
        Action::PlayerCounter {
            seat,
            name,
            adjustment,
        } => {
            format!(
                "{} for {}",
                counter_change(name, *adjustment),
                player(*seat)
            )
        }
        Action::Phase { phase } => format!("Moved to {}", phase_name(*phase)),
        Action::NextTurn {} => {
            let seat = summary.turn_seat.unwrap_or(names.after.active);
            if seat == viewer {
                "Started your turn".into()
            } else {
                format!("Started the turn for {}", player(seat))
            }
        }
        Action::Roll { sides, .. } => roll(*sides, &summary.dice),
        Action::Arrow { selection, to } => {
            let target = match to {
                Target::Player(seat) => player(*seat),
                Target::Object(seen) => names.object(seen.id).to_owned(),
            };
            format!(
                "Drew an arrow from {} to {target}",
                names.selected(selection)
            )
        }
        Action::ClearArrows { .. } => "Cleared arrows".into(),
        Action::LoadDeck { .. } => {
            format!("Loaded a deck ({} in sideboard)", summary.sideboard_size)
        }
        Action::Sideboard { .. } => "Sideboarded".into(),
        Action::Ready { ready: true, .. } => "Ready".into(),
        Action::Ready { ready: false, .. } => "Cleared readiness".into(),
        Action::GameSettings { .. } => "Changed game settings".into(),
        Action::Start {
            first,
            opening: true,
        } => {
            if *first == viewer {
                "Dealt opening hands. You play first".into()
            } else {
                format!("Dealt opening hands. {} plays first", player(*first))
            }
        }
        Action::Start { .. } => "Started the game".into(),
        Action::Keep {} => "Kept the opening hand".into(),
        Action::NextGame {
            winner: Some(winner),
        } => format!("Ended the game. {} won", player(*winner)),
        Action::NextGame { winner: None } => "Ended the game. No result".into(),
        Action::Concede {} => "Conceded".into(),
        Action::SetAssist { on: true, .. } => "Enabled an assist".into(),
        Action::SetAssist { on: false, .. } => "Disabled an assist".into(),
        Action::CloseLook {} => "Closed the pile viewer".into(),
        Action::Flow { .. } => "Opened a library flow".into(),
        Action::Undo { .. } => "Undid an action".into(),
        Action::Rewind { to } => format!("Rewound to entry {to}"),
        Action::Batch { actions } if paired_rolls(actions) => summary
            .dice
            .as_chunks::<2>()
            .0
            .iter()
            .map(|dice| roll(6, dice))
            .collect::<Vec<_>>()
            .join(". "),
        Action::Batch { actions } => match actions.as_slice() {
            [action @ (Action::Start { opening: true, .. } | Action::Mulligan { .. })] => {
                describe(action, summary, names)
            }
            [Action::Keep {}, rest @ ..] => {
                opening_keep(rest).unwrap_or_else(|| batch_line(actions.len()))
            }
            _ => batch_line(actions.len()),
        },
        _ => unreachable!("expected a game action"),
    }
}

fn batch_line(count: usize) -> String {
    format!("Applied a batch of {}", amount(count, "action", "actions"))
}

fn opening_keep(actions: &[Action]) -> Option<String> {
    let mut text = "Kept the opening hand".to_owned();
    for action in actions {
        match action {
            Action::Move {
                selection,
                to: Destination::Zone(Zone::Library(seat)),
                position: Position::Bottom,
                ..
            } if selection.zone == Zone::Hand(*seat) => {
                // The count is public. The cards stay private.
                text.push_str(". Put ");
                text.push_str(&amount(selection.objects.len(), "card", "cards"));
                text.push_str(" on the bottom");
            }
            Action::Phase {
                phase: Phase::Untap,
            } => text.push_str(". Moved to untap"),
            _ => return None,
        }
    }
    Some(text)
}

fn paired_rolls(actions: &[Action]) -> bool {
    !actions.is_empty()
        && actions
            .iter()
            .all(|action| matches!(action, Action::Roll { count: 2, sides: 6 }))
}

fn movement(
    selection: &Selection,
    destination: Destination,
    position: Position,
    face: Option<Face>,
    summary: &Summary,
    names: &Cards<'_>,
) -> String {
    let viewer = names.viewer;
    let from = zone_name(selection.zone, viewer);
    let subject = if face == Some(Face::Down) {
        format!("{} face down", card_count(selection.objects.len()))
    } else if let Destination::Zone(Zone::Hand(owner)) = destination
        && selection.zone != Zone::Hand(owner)
        && owner != viewer
        && selection.objects.len() == 1
    {
        // The hand entry replaces the visible id. Keep the flight and its
        // history anonymous for seats that cannot see the new identity.
        "a card".into()
    } else if let [id] = selection.objects.as_slice()
        && matches!(destination, Destination::Computed(_))
    {
        names.object(*id).to_owned()
    } else {
        names.selected(selection)
    };
    match destination {
        Destination::Computed(ComputedDestination::Play) if summary.cast => {
            format!("Cast {subject}")
        }
        Destination::Computed(ComputedDestination::Play) => format!("Played {subject} from {from}"),
        Destination::Computed(ComputedDestination::Resolve) => {
            format!("Resolved {subject} from {from}")
        }
        Destination::Zone(zone) => format!(
            "Put {subject} from {from} {}",
            move_destination(zone, position, viewer)
        ),
    }
}

struct Cards<'a> {
    before: &'a Table,
    after: &'a Table,
    viewer: Seat,
    recorded_names: Option<&'a RefCell<Vec<String>>>,
    anonymous: bool,
}

impl Cards<'_> {
    fn selected(&self, selection: &Selection) -> String {
        match selection.objects.as_slice() {
            [id] if selection.zone.is_public() => self.object(*id).to_owned(),
            [_] => "a card".into(),
            objects => card_count(objects.len()),
        }
    }

    fn counter_label<'label>(&self, selection: &Selection, label: &'label str) -> &'label str {
        if crate::redact::private_counter_name(
            selection,
            label,
            self.before,
            self.after,
            self.viewer,
        ) {
            return "";
        }
        for id in &selection.objects {
            let Some(object) = self
                .after
                .objects
                .get(id)
                .or_else(|| self.before.objects.get(id))
            else {
                continue;
            };
            if let Some(name) = object.card.name()
                && !name.is_empty()
                && label.to_lowercase().contains(&name.to_lowercase())
            {
                if self.anonymous {
                    return "";
                }
                self.record_name(name);
            }
        }
        label
    }

    fn record_name(&self, name: &str) {
        if let Some(names) = self.recorded_names {
            names.borrow_mut().push(name.to_owned());
        }
    }

    fn object(&self, id: ObjectId) -> &str {
        if !self.anonymous
            && let Some(object) = self
                .after
                .objects
                .get(&id)
                .or_else(|| self.before.objects.get(&id))
            && object.known_to.contains(&self.viewer)
            && let Some(name) = object.card.name()
        {
            self.record_name(name);
            return name;
        }
        if self.after.markers.contains_key(&id) || self.before.markers.contains_key(&id) {
            "an ability"
        } else {
            "a card"
        }
    }
}

fn amount(count: usize, singular: &str, plural: &str) -> String {
    let noun = if count == 1 { singular } else { plural };
    format!("{count} {noun}")
}

fn card_count(count: usize) -> String {
    amount(count, "card", "cards")
}

fn player_name(seat: Seat, viewer: Seat) -> String {
    if seat == viewer {
        "you".into()
    } else {
        format!("Seat {}", u16::from(seat.0) + 1)
    }
}

fn zone_name(zone: Zone, viewer: Seat) -> String {
    let (seat, label) = match zone {
        Zone::Hand(seat) => (seat, "hand"),
        Zone::Library(seat) => (seat, "library"),
        Zone::Graveyard(seat) => (seat, "graveyard"),
        Zone::Exile(seat) => (seat, "exile"),
        Zone::Sideboard(seat) => (seat, "sideboard"),
        Zone::Stack => return "the stack".into(),
        Zone::Battlefield => return "the battlefield".into(),
    };
    if seat == viewer {
        format!("your {label}")
    } else {
        format!("{}'s {label}", player_name(seat, viewer))
    }
}

fn move_destination(zone: Zone, position: Position, viewer: Seat) -> String {
    let pile = zone_name(zone, viewer);
    match (zone, position) {
        (Zone::Library(_), Position::Top | Position::Nth(0)) => format!("on top of {pile}"),
        (Zone::Library(_), Position::Bottom) => format!("on the bottom of {pile}"),
        (Zone::Library(_), Position::Nth(n)) => format!("{n} cards from the top of {pile}"),
        _ => format!("into {pile}"),
    }
}

fn part(zone: Zone, portion: Portion, viewer: Seat) -> String {
    let pile = zone_name(zone, viewer);
    match portion {
        Portion::All => pile,
        Portion::Top(count) => format!("the top {} of {pile}", card_count(usize::from(count))),
        Portion::Bottom(count) => {
            format!("the bottom {} of {pile}", card_count(usize::from(count)))
        }
    }
}

fn phase_name(phase: Phase) -> &'static str {
    match phase {
        Phase::Untap => "untap",
        Phase::Upkeep => "upkeep",
        Phase::Draw => "draw",
        Phase::FirstMain => "first main",
        Phase::BeginningOfCombat => "beginning of combat",
        Phase::DeclareAttackers => "declare attackers",
        Phase::DeclareBlockers => "declare blockers",
        Phase::CombatDamage => "combat damage",
        Phase::EndOfCombat => "end of combat",
        Phase::SecondMain => "second main",
        Phase::End => "end step",
    }
}

fn counter_change(name: &str, adjustment: Adjustment) -> String {
    let name = if name.is_empty() { "a" } else { name };
    match adjustment {
        Adjustment::Add(count) if count > 0 => format!("Added {count} {name} counters"),
        Adjustment::Add(count) => format!("Removed {} {name} counters", count.unsigned_abs()),
        Adjustment::Set(count) => format!("Set {name} counters to {count}"),
    }
}

fn roll(sides: u32, outcomes: &[u32]) -> String {
    if outcomes.is_empty() {
        return if sides == 2 {
            "Flipped a coin"
        } else {
            "Rolled dice"
        }
        .into();
    }
    if sides == 2 {
        let results: Vec<_> = outcomes
            .iter()
            .map(|&outcome| if outcome == 0 { "heads" } else { "tails" })
            .collect();
        let coins = if results.len() == 1 {
            "a coin"
        } else {
            "coins"
        };
        return format!("Flipped {coins}: {}", results.join(", "));
    }
    let results: Vec<_> = outcomes
        .iter()
        .map(|outcome| (outcome + 1).to_string())
        .collect();
    let dice = if results.len() == 1 { "a die" } else { "dice" };
    format!("Rolled {dice}: {}", results.join(", "))
}
