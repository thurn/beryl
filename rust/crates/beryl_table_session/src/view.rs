//! Builds the stage's view and its matching action references for one seat.

mod cards;
mod slots;

pub use slots::Slots;

use beryl_session::protocol::{
    AssistsView, LogLine, ManualOrders, ManualPiles, NamedCounter, ObjectId as Slot, PlayerState,
    PlayerView, Side, StackItem, Step, TableActionRefs, TableArrow, TablePhase, TableView,
    ZoneName, ZoneRef,
};
use beryl_session::script::{Arrow, Subject};
use beryl_table::assist;
use beryl_table::log::UndoControl;
use beryl_table::state::{ArrowKind, ArrowTarget, Assist, ObjectId, Phase, Zone};
use beryl_table::{Log, Refusal, Seat, Table, redact};

/// Publish this view and retain its action references together. Use the same
/// slots for successive views of this seat. The log supplies both the wording
/// captured for this seat and the changes that can preserve a card's slot.
pub fn view(
    table: &Table,
    log: &Log,
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
        .update(&projected, seat, log.entries())
        .ok_or(Refusal::INVALID)?;

    let (you, yours) = piles(&projected, seat, slots);
    let (opponent, theirs) = piles(&projected, Seat(1 - seat.0), slots);
    let mut view = PlayerView::neutral(you, opponent);
    let mut refs = TableActionRefs::new(table, seat);
    for (&id, object) in &projected.objects {
        let slot = slots.get(id).expect("slot assigned to each displayed card");
        view.objects
            .push(cards::object(object, slot, slots, viewer));
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

    let (phase, step) = phase(projected.phase);
    view.turn.step = step;
    view.turn.active = viewer.side(projected.active);
    view.turn.number = u16::try_from(projected.turn).unwrap_or(u16::MAX);
    let rounds = projected.turn.div_ceil(projected.players.len() as u32);
    view.turn.own_number = u16::try_from(rounds).unwrap_or(u16::MAX);
    view.table = Some(TableView {
        log: history(log, table, seat),
        proposals: Vec::new(),
        labels: Vec::new(),
        assists: assists(&projected, seat),
        phase,
        seats: projected.players.len() as u8,
        orders: ManualOrders {
            you: yours,
            opponent: theirs,
        },
        arrows: arrows(&projected, slots, viewer),
        acting: acting(&projected, viewer),
        object_actions: refs.actions().to_vec(),
        prompt: None,
    });
    Ok((view, refs))
}

#[derive(Clone, Copy)]
struct Viewer(Seat);

impl Viewer {
    fn side(self, seat: Seat) -> Side {
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

fn piles(table: &Table, seat: Seat, slots: &Slots) -> (PlayerState, ManualPiles) {
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
    for &(lent, to) in &table.lent_zones {
        if to == viewer.0
            && let (zone, Some(seat)) = zone(lent)
        {
            let item = ZoneRef {
                side: viewer.side(seat),
                zone,
            };
            if !zones.contains(&item) {
                zones.push(item);
            }
        }
    }
    for seat in table.seats().filter(|&seat| seat != viewer.0) {
        let hand = ZoneRef {
            side: viewer.side(seat),
            zone: ZoneName::Hand,
        };
        if !zones.contains(&hand)
            && table.zones[usize::from(seat.0)]
                .hand
                .iter()
                .any(|id| table.objects[id].known_to.contains(&viewer.0))
        {
            zones.push(hand);
        }
    }
    zones
}

fn phase(phase: Phase) -> (TablePhase, Step) {
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

fn arrows(table: &Table, slots: &Slots, viewer: Viewer) -> Vec<TableArrow> {
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
            Some(TableArrow { from, to, arrow })
        })
        .collect()
}
