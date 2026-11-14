use serde::{Deserialize, Serialize};

use crate::action::{
    Adjustment, ComputedDestination, Destination, Portion, Position, Selection, Target,
};
use crate::state::{Card, Face, ObjectId, Phase, Zone};
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
    let mut text = if let Some(action) = &entry.undone_action {
        let description = describe(action, entry, table, for_seat);
        format!("Undid: {description}")
    } else {
        describe(&entry.action, entry, table, for_seat)
    };
    if entry.seen {
        text.push_str(" (seen)");
    }
    text.push('.');
    text
}

fn describe(action: &Action, entry: &Entry, table: &Table, viewer: Seat) -> String {
    let cards = |selection: &Selection| selected(selection, table, viewer);
    let pile = |zone| zone_name(zone, viewer);
    let player = |seat| player_name(seat, viewer);
    match action {
        Action::Move {
            selection,
            to,
            position,
            face,
            ..
        } => movement(
            selection,
            *to,
            *position,
            *face,
            &entry.summary,
            table,
            viewer,
        ),
        Action::Reorder { selection, .. } => {
            format!("Reordered {} in {}", cards(selection), pile(selection.zone))
        }
        Action::Pin { selection, .. } => format!("Changed the row for {}", cards(selection)),
        Action::Give { selection, to } => {
            format!("Gave control of {} to {}", cards(selection), player(*to))
        }
        Action::Tap { tapped: false, .. } if entry.summary.untapped_all => {
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
            format!(
                "Attached {} to {}",
                cards(selection),
                object_name(table, host.id, viewer)
            )
        }
        Action::Unattach { selection } => format!("Detached {}", cards(selection)),
        Action::Clone { selection } => format!("Created a token copy of {}", cards(selection)),
        Action::Token { count, .. } => {
            format!("Created {}", amount(usize::from(*count), "token", "tokens"))
        }
        Action::Marker { .. } => "Put an ability on the stack".into(),
        Action::Remove { selection } => format!("Removed {}", cards(selection)),
        Action::Draw { .. }
        | Action::Mulligan { .. }
        | Action::Shuffle { .. }
        | Action::Look { .. }
        | Action::Reveal { .. }
        | Action::Lend { .. }
        | Action::AlwaysRevealTop { .. }
        | Action::AlwaysLookAtTop { .. }
        | Action::SortHand { .. } => library(action, &entry.summary, viewer),
        _ => game(action, &entry.summary, table, viewer),
    }
}

fn library(action: &Action, summary: &Summary, viewer: Seat) -> String {
    let pile = |zone| zone_name(zone, viewer);
    let player = |seat| player_name(seat, viewer);
    match action {
        Action::Draw { .. } if summary.count == 0 => "Drew from an empty library".into(),
        Action::Draw { .. } => format!("Drew {}", card_count(summary.count)),
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

fn game(action: &Action, summary: &Summary, table: &Table, viewer: Seat) -> String {
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
            let seat = summary.turn_seat.unwrap_or(table.active);
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
                Target::Object(seen) => object_name(table, seen.id, viewer).to_owned(),
            };
            format!(
                "Drew an arrow from {} to {target}",
                selected(selection, table, viewer)
            )
        }
        Action::ClearArrows { .. } => "Cleared arrows".into(),
        Action::LoadDeck { .. } => {
            format!("Loaded a deck ({} in sideboard)", summary.sideboard_size)
        }
        Action::Sideboard { .. } => "Sideboarded".into(),
        Action::Ready { ready: true, .. } => "Ready".into(),
        Action::Ready { ready: false, .. } => "Cleared readiness".into(),
        Action::Start { .. } => "Started the game".into(),
        Action::NextGame {
            winner: Some(winner),
        } => format!("Ended the game. {} won", player(*winner)),
        Action::NextGame { winner: None } => "Ended the game".into(),
        Action::Concede {} => "Conceded".into(),
        Action::SetAssist { on: true, .. } => "Enabled an assist".into(),
        Action::SetAssist { on: false, .. } => "Disabled an assist".into(),
        Action::Flow { .. } => "Opened a library flow".into(),
        Action::Undo { .. } => "Undid an action".into(),
        Action::Rewind { to } => format!("Rewound to entry {to}"),
        Action::Batch { actions } => format!(
            "Applied a batch of {}",
            amount(actions.len(), "action", "actions")
        ),
        _ => unreachable!("expected a game action"),
    }
}

fn movement(
    selection: &Selection,
    destination: Destination,
    position: Position,
    face: Option<Face>,
    summary: &Summary,
    table: &Table,
    viewer: Seat,
) -> String {
    let from = zone_name(selection.zone, viewer);
    let subject = if face == Some(Face::Down) {
        format!("{} face down", card_count(selection.objects.len()))
    } else if let [id] = selection.objects.as_slice()
        && matches!(destination, Destination::Computed(_))
    {
        object_name(table, *id, viewer).to_owned()
    } else {
        selected(selection, table, viewer)
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

fn selected(selection: &Selection, table: &Table, viewer: Seat) -> String {
    if let [id] = selection.objects.as_slice()
        && selection.zone.is_public()
    {
        return object_name(table, *id, viewer).to_owned();
    }
    if selection.objects.len() == 1 {
        "a card".into()
    } else {
        card_count(selection.objects.len())
    }
}

fn object_name(table: &Table, id: ObjectId, viewer: Seat) -> &str {
    if let Some(object) = table.objects.get(&id)
        && object.known_to.contains(&viewer)
        && let Card::Named(name) | Card::Custom { name, .. } = &object.card
    {
        return name;
    }
    if table.markers.contains_key(&id) {
        "an ability"
    } else {
        "a card"
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
