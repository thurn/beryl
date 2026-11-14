use beryl_card_store::facts;

use crate::action::{Adjustment, SeenObject, Selection, Target};
use crate::change::{Edit, TableEdit};
use crate::state::{Arrow, ArrowKind, ArrowTarget, Card, Face, Object, Phase, Player, Zone};
use crate::{Chance, Change, Refusal, Seat, Table};

use super::{Changes, cards};

pub(super) fn player(table: &Table, seat: Seat) -> Result<&Player, Refusal> {
    table
        .players
        .get(usize::from(seat.0))
        .ok_or(Refusal::Invalid)
}

pub(super) fn set_life(seat: Seat, before: i32, after: i32) -> Option<Change> {
    (before != after).then_some(Change::SetTable(TableEdit::SetLife {
        seat,
        edit: Edit { before, after },
    }))
}

pub(super) fn life(table: &Table, seat: Seat, adjustment: Adjustment) -> Changes {
    let before = player(table, seat)?.life;
    Ok(set_life(seat, before, adjustment.apply(before))
        .into_iter()
        .collect())
}

pub(super) fn player_counter(
    table: &Table,
    seat: Seat,
    name: &str,
    adjustment: Adjustment,
) -> Changes {
    let counters = &player(table, seat)?.counters;
    Ok(cards::counter_edit(counters, name, adjustment, i32::MAX)
        .map(|edit| Change::SetTable(TableEdit::SetPlayerCounters { seat, edit }))
        .into_iter()
        .collect())
}

pub(super) fn roll(count: u16, sides: u32, chance: &mut Chance) -> Changes {
    if !(1..=100).contains(&count) || !(2..=1_000_000).contains(&sides) {
        return Err(Refusal::Invalid);
    }
    let outcomes = (0..count)
        .map(|_| chance.below(sides))
        .collect::<Result<_, _>>()?;
    Ok(vec![Change::Random { outcomes }])
}

fn target_card(table: &Table, seen: SeenObject) -> Result<&Object, Refusal> {
    cards::validate_seen(table, seen)?;
    table.objects.get(&seen.id).ok_or(Refusal::Invalid)
}

fn target(table: &Table, to: Target) -> Result<ArrowTarget, Refusal> {
    Ok(match to {
        Target::Object(seen) => ArrowTarget::Object(target_card(table, seen)?.id),
        Target::Player(seat) => {
            player(table, seat)?;
            ArrowTarget::Player(seat)
        }
    })
}

fn has_type(object: &Object, name: &str, front: fn(&beryl_card_store::Record) -> bool) -> bool {
    // Using the printed type of a face-down card would leak it through the arrow.
    if object.zone != Zone::Battlefield || object.face == Face::Down {
        return false;
    }
    let record;
    let types = match &object.card {
        Card::Custom { types, .. } => types,
        Card::Hidden => return false,
        Card::Named(_) => {
            record = cards::card_record(&object.card);
            let Some(record) = &record else { return false };
            if object.face == Face::Front {
                return front(record);
            }
            let Some(face) = cards::printed_face(record, object.face) else {
                return false;
            };
            &face.type_line
        }
    };
    types
        .split('—')
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .any(|word| word == name)
}

fn arrow_kind(table: &Table, from: &Object, to: ArrowTarget) -> ArrowKind {
    let (controller, target_card) = match to {
        ArrowTarget::Object(id) => {
            let card = &table.objects[&id];
            (card.controller, Some(card))
        }
        ArrowTarget::Player(seat) => (seat, None),
    };
    let friendly = from.controller == controller;
    if has_type(from, "Creature", facts::is_creature) {
        if !friendly
            && matches!(
                table.phase,
                Phase::BeginningOfCombat
                    | Phase::DeclareAttackers
                    | Phase::DeclareBlockers
                    | Phase::CombatDamage
            )
            && target_card.is_none_or(|card| has_type(card, "Planeswalker", facts::is_planeswalker))
        {
            return ArrowKind::Attack;
        }
        if target_card.is_some_and(|card| {
            has_type(card, "Creature", facts::is_creature)
                && table
                    .arrows
                    .iter()
                    .any(|arrow| arrow.from == card.id && arrow.kind == ArrowKind::Attack)
        }) {
            return ArrowKind::Block;
        }
    }
    if friendly {
        ArrowKind::FriendlyTarget
    } else {
        ArrowKind::HostileTarget
    }
}

pub(super) fn arrow(table: &Table, by: Seat, selection: &Selection, to: Target) -> Changes {
    player(table, by)?;
    let to = target(table, to)?;
    Ok(cards::selected_cards(table, selection)?
        .into_iter()
        .enumerate()
        .map(|(offset, card)| Change::InsertArrow {
            arrow: Arrow {
                from: card.id,
                to,
                by,
                kind: arrow_kind(table, card, to),
            },
            index: table.arrows.len() + offset,
        })
        .collect())
}

pub(super) fn remove_arrows<'a>(
    table: &'a Table,
    matches: impl Fn(&Arrow) -> bool + 'a,
) -> impl Iterator<Item = Change> + 'a {
    table
        .arrows
        .iter()
        .enumerate()
        .rev()
        .filter(move |(_, arrow)| matches(arrow))
        .map(|(index, arrow)| Change::RemoveArrow {
            arrow: arrow.clone(),
            index,
        })
}

pub(super) fn clear_arrows(
    table: &Table,
    by: Seat,
    from: Option<SeenObject>,
    to: Option<Target>,
) -> Changes {
    player(table, by)?;
    let from = from
        .map(|seen| target_card(table, seen).map(|card| card.id))
        .transpose()?;
    let to = to.map(|to| target(table, to)).transpose()?;
    Ok(remove_arrows(table, |arrow| {
        arrow.by == by
            && from.is_none_or(|id| arrow.from == id)
            && to.is_none_or(|target| arrow.to == target)
    })
    .collect())
}
