//! Builds the stage's view and its matching action references for one seat.

pub(super) mod cards;
mod slots;

pub(super) use slots::Slots;

use std::collections::{BTreeMap, HashSet};

use beryl_card_store::{STORE, facts};
use beryl_session::ipc::decks::CardTokenLink;
use beryl_session::protocol::{
    AssistsView, CardHandling, LogLine, ManualOrders, ManualPiles, NamedCounter, ObjectId as Slot,
    PlayerState, PlayerView, Side, StackItem, Step, TableActionRefs, TableArrow, TablePhase,
    TableView, ZoneName, ZoneRef,
};
use beryl_session::script::{Arrow, Subject, Turn};
use beryl_table::action::Portion;
use beryl_table::assist;
use beryl_table::log::UndoControl;
use beryl_table::state::{ArrowKind, ArrowTarget, Assist, ObjectId, Phase, Zone};
use beryl_table::{Entry, Log, Refusal, Seat, Table, permissions, redact};

pub(super) fn project(
    table: &Table,
    entries: &[Entry],
    seat: Seat,
    slots: &mut Slots,
) -> Result<(PlayerView, TableActionRefs), Refusal> {
    let projected = redact::seen_by(table, seat);
    let viewer = Viewer(seat);
    if !(1..=2).contains(&projected.players.len())
        || projected.players.get(usize::from(seat.0)).is_none()
    {
        return Err(Refusal::INVALID);
    }
    slots
        .update(&projected, seat, entries)
        .ok_or(Refusal::INVALID)?;

    let (you, yours) = piles(&projected, seat, slots);
    let (opponent, theirs) = piles(&projected, Seat(1 - seat.0), slots);
    let mut view = PlayerView::neutral(you, opponent);
    let mut refs = TableActionRefs::new(table, seat);
    let mut card_commands = BTreeMap::new();
    for (&id, object) in &projected.objects {
        let slot = slots.get(id).expect("slot assigned to each displayed card");
        let card = cards::CardProjection::new(object);
        view.objects.push(card.object(slot, slots, viewer));
        if let Some(commands) = card.commands() {
            card_commands.insert(slot, commands);
        }
        refs.expose(table, Subject::Object { slot }, id);
    }

    view.battlefield = order(&projected.battlefield, slots);
    for &id in projected.stack.iter().rev() {
        if let Some(marker) = projected.markers.get(&id) {
            view.stack.push(cards::marker(id, marker, slots, viewer));
            refs.expose(table, Subject::Ability { id: id.0 }, id);
        } else if let Some(slot) = slots.get(id) {
            view.stack.push(StackItem::spell(slot));
        }
    }

    let (phase, turn) = turn(&projected, viewer);
    view.turn.step = turn.step;
    view.turn.active = turn.active;
    view.turn.number = turn.number;
    view.turn.own_number = turn.own_number;
    view.handling = handling(&projected, viewer);
    view.table = Some(TableView {
        log: Vec::new(),
        proposals: Vec::new(),
        labels: Vec::new(),
        assists: assists(&projected, seat),
        phase,
        seats: projected.players.len() as u8,
        seat: (seat.0 != 0).then_some(seat.0),
        orders: ManualOrders {
            you: yours,
            opponent: theirs,
        },
        arrows: arrows(&projected, slots, viewer),
        acting: acting(table, viewer),
        object_actions: refs.actions().to_vec(),
        card_commands,
        top_revealed: projected.always_reveal_top.contains(&seat),
        top_known: projected.always_look_at_top.contains(&seat),
        manual_stats: projected
            .objects
            .values()
            .filter(|card| card.stats.is_some() && card.zone == Zone::Battlefield)
            .filter_map(|card| slots.get(card.id))
            .collect(),
        last_draw: None,
        deck_tokens: deck_tokens(&projected, seat),
        prompt: None,
    });
    Ok((view, refs))
}

fn deck_tokens(table: &Table, seat: Seat) -> Vec<CardTokenLink> {
    let mut seen = HashSet::new();
    table.game_match.seats[usize::from(seat.0)]
        .deck
        .iter()
        .filter_map(|(name, _)| STORE.card_ignoring_case(name))
        .flat_map(|record| facts::tokens_made(&record).to_vec())
        .filter(|token| seen.insert(token.scryfall_id.clone()))
        .take(10)
        .map(|token| CardTokenLink {
            name: token.name,
            scryfall_id: token.scryfall_id,
        })
        .collect()
}

#[derive(Clone, Copy)]
pub(super) struct Viewer(pub Seat);

impl Viewer {
    pub(super) fn side(self, seat: Seat) -> Side {
        if self.0 == seat {
            Side::You
        } else {
            Side::Opponent
        }
    }
}

fn order(ids: &[ObjectId], slots: &Slots) -> Vec<Slot> {
    ids.iter().filter_map(|&id| slots.get(id)).collect()
}

pub(super) fn piles(table: &Table, seat: Seat, slots: &Slots) -> (PlayerState, ManualPiles) {
    let index = usize::from(seat.0);
    let Some(player) = table.players.get(index) else {
        return (PlayerState::neutral(0), ManualPiles::default());
    };
    let mut state = PlayerState::neutral(player.life);
    let zones = &table.zones[index];
    state.library = order(&zones.library, slots);
    state.library.reverse();
    state.graveyard = order(&zones.graveyard, slots);
    state.graveyard.reverse();
    for (name, count) in &player.counters {
        let mana = match name.as_str() {
            "W" => &mut state.mana.white,
            "U" => &mut state.mana.blue,
            "B" => &mut state.mana.black,
            "R" => &mut state.mana.red,
            "G" => &mut state.mana.green,
            "C" => &mut state.mana.colorless,
            _ => {
                state.named_counters.push(NamedCounter {
                    name: name.clone(),
                    count: *count,
                });
                continue;
            }
        };
        *mana = u32::try_from(*count).unwrap_or_default();
    }
    let mut piles = ManualPiles {
        hand: order(&zones.hand, slots),
        exile: order(&zones.exile, slots),
        sideboard: order(&zones.sideboard, slots),
    };
    piles.exile.reverse();
    piles.sideboard.reverse();
    (state, piles)
}

pub(super) fn zone(zone: Zone) -> (ZoneName, Option<Seat>) {
    match zone {
        Zone::Library(seat) => (ZoneName::Library, Some(seat)),
        Zone::Hand(seat) => (ZoneName::Hand, Some(seat)),
        Zone::Graveyard(seat) => (ZoneName::Graveyard, Some(seat)),
        Zone::Exile(seat) => (ZoneName::Exile, Some(seat)),
        Zone::Sideboard(seat) => (ZoneName::Sideboard, Some(seat)),
        Zone::Battlefield => (ZoneName::Battlefield, None),
        Zone::Stack => (ZoneName::Stack, None),
    }
}

fn acting(table: &Table, viewer: Viewer) -> Vec<ZoneRef> {
    let mut zones = Vec::new();
    for lent in permissions::zone_grants(table, viewer.0) {
        if let (zone, Some(seat)) = zone(lent) {
            let item = ZoneRef {
                side: viewer.side(seat),
                zone,
            };
            if !zones.contains(&item) {
                zones.push(item);
            }
        }
    }
    zones
}

pub(super) fn handling(table: &Table, viewer: Viewer) -> Option<CardHandling> {
    let handling = table.handling?;
    let (pile, owner) = zone(handling.zone);
    let len = table.zone(handling.zone)?.len();
    let count = match handling.portion {
        Portion::All => len,
        Portion::Top(n) | Portion::Bottom(n) => len.min(usize::from(n)),
    };
    (count != 0).then(|| CardHandling::Pile {
        side: viewer.side(owner.expect("a handled pile has an owner")),
        pile,
        count: u32::try_from(count).expect("a pile contains at most one card per object id"),
        // An observer sees the count as a packet of backs. Only the player
        // looking needs its positions in the library.
        bottom: handling.by == viewer.0 && matches!(handling.portion, Portion::Bottom(_)),
    })
}

pub(super) fn turn(table: &Table, viewer: Viewer) -> (TablePhase, Turn) {
    let (phase, step) = phase(table.phase);
    let rounds = table.turn.div_ceil(table.players.len() as u32);
    (
        phase,
        Turn {
            number: u16::try_from(table.turn).unwrap_or(u16::MAX),
            own_number: u16::try_from(rounds).unwrap_or(u16::MAX),
            active: viewer.side(table.active),
            step,
        },
    )
}

pub(super) fn phase(phase: Phase) -> (TablePhase, Step) {
    match phase {
        Phase::Untap => (TablePhase::Untap, Step::Untap),
        Phase::Upkeep => (TablePhase::Upkeep, Step::Upkeep),
        Phase::Draw => (TablePhase::Draw, Step::Draw),
        Phase::FirstMain => (TablePhase::FirstMain, Step::PrecombatMain),
        Phase::BeginningOfCombat => (TablePhase::BeginningOfCombat, Step::BeginningOfCombat),
        Phase::DeclareAttackers => (TablePhase::DeclareAttackers, Step::DeclareAttackers),
        Phase::DeclareBlockers => (TablePhase::DeclareBlockers, Step::DeclareBlockers),
        Phase::CombatDamage => (TablePhase::CombatDamage, Step::CombatDamage),
        Phase::EndOfCombat => (TablePhase::EndOfCombat, Step::EndOfCombat),
        Phase::SecondMain => (TablePhase::SecondMain, Step::PostcombatMain),
        Phase::End => (TablePhase::End, Step::End),
    }
}

fn assists(table: &Table, seat: Seat) -> AssistsView {
    let on = |setting| assist::enabled(table, seat, setting);
    AssistsView {
        turn_upkeep: on(Assist::TurnUpkeep),
        combat: on(Assist::Combat),
        state_nudges: on(Assist::StateNudges),
        library_flows: on(Assist::LibraryFlows),
        ability_menus: on(Assist::AbilityMenus),
    }
}

// Log controls depend on the recorded entry, including strikes and undo targets.
pub(super) fn attach_history(view: &mut PlayerView, log: &Log, table: &Table, seat: Seat) {
    let manual = view.table.as_mut().expect("manual table projection");
    manual.log = history(log, table, seat);
    manual.last_draw = log.last_draw(table, seat);
}

fn history(log: &Log, table: &Table, seat: Seat) -> Vec<LogLine> {
    log.visible_history(table, seat)
        .map(|row| LogLine {
            n: row.n,
            by: if row.by == seat {
                "You".to_owned()
            } else {
                format!("Seat {}", u16::from(row.by.0) + 1)
            },
            text: row.text.to_owned(),
            struck: row.struck,
            undoable: matches!(row.undo, Some(UndoControl::Available)),
            blocked_by: match row.undo {
                Some(UndoControl::Blocked(entries)) => entries,
                _ => Vec::new(),
            },
        })
        .collect()
}

pub(super) fn arrows(table: &Table, slots: &Slots, viewer: Viewer) -> Vec<TableArrow> {
    table
        .arrows
        .iter()
        .filter_map(|entry| {
            let from = Subject::Object {
                slot: slots.get(entry.from)?,
            };
            let to = match entry.to {
                ArrowTarget::Object(id) => Subject::Object {
                    slot: slots.get(id)?,
                },
                ArrowTarget::Player(seat) => Subject::Player {
                    player: viewer.side(seat),
                },
            };
            let arrow = match entry.kind {
                ArrowKind::Attack => Arrow::Attack,
                ArrowKind::Block => Arrow::Block,
                ArrowKind::FriendlyTarget => Arrow::FriendlyTarget,
                ArrowKind::HostileTarget => Arrow::HostileTarget,
            };
            Some(TableArrow {
                from,
                to,
                arrow,
                own: entry.by == viewer.0,
            })
        })
        .collect()
}
