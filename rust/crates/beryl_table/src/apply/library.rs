use std::collections::BTreeSet;
use std::ops::Range;

use beryl_card_store::{Record, facts};

use crate::action::{Destination, Portion, Position, Selection, SortBy};
use crate::change::{Edit, TableEdit};
use crate::helpers::card_record;
use crate::state::{Card, Handling, Object, ObjectId, Zone};
use crate::{Action, Chance, Change, Refusal, Seat, Table};

use super::{AppliedChanges, Changes, Presentation, cards, perform};

super::unimplemented_actions!(flow);

/// Closing a viewer doesn't revoke permission to use a lent pile or forget
/// cards the player has seen. A changed pile drops its lift before its cards
/// move, so a newly arrived card cannot inherit a place in the lifted group.
pub(super) fn settle_handling(
    table: &Table,
    by: Seat,
    action: &Action,
    applied: &mut AppliedChanges,
) {
    let before = table.handling;
    let open = |zone, portion| Some(Handling { by, zone, portion });
    let after = match *action {
        Action::Look { seat, portion } => {
            let zone = Zone::Library(seat);
            let len = table.zone(zone).expect("validated library").len();
            if portion_range(len, portion).is_empty() {
                None
            } else {
                open(zone, portion)
            }
        }
        Action::Lend { zone, .. } if table.zone(zone).is_some_and(|ids| !ids.is_empty()) => {
            open(zone, Portion::All)
        }
        Action::CloseLook {} if before.is_some_and(|handling| handling.by == by) => None,
        Action::NextTurn {} | Action::Start { .. } | Action::NextGame { .. } => None,
        _ => {
            if let Some(handling) = before
                && let Some(index) = applied.changes.iter().position(|change| match change {
                    Change::Move { from, to, .. } => {
                        from.zone == handling.zone || to.zone == handling.zone
                    }
                    Change::Create { object, .. } | Change::Remove { object, .. } => {
                        object.zone == handling.zone
                    }
                    Change::ReplaceId { from, .. } => table
                        .objects
                        .get(from)
                        .is_some_and(|object| object.zone == handling.zone),
                    _ => false,
                })
            {
                applied.close_handling_before(index, handling_change(before, None));
            }
            return;
        }
    };
    if before != after {
        applied.changes.push(handling_change(before, after));
        applied.extend_last_member(1);
    }
}

fn handling_change(before: Option<Handling>, after: Option<Handling>) -> Change {
    Change::SetTable(TableEdit::SetHandling {
        edit: Edit { before, after },
    })
}

fn zone_cards(table: &Table, zone: Zone) -> Result<&[ObjectId], Refusal> {
    table.zone(zone).ok_or(Refusal::INVALID)
}

fn portion_range(len: usize, portion: Portion) -> Range<usize> {
    match portion {
        Portion::All => 0..len,
        Portion::Top(count) => 0..len.min(usize::from(count)),
        Portion::Bottom(count) => len.saturating_sub(usize::from(count))..len,
    }
}

fn move_to(table: &Table, from: Zone, ids: Vec<ObjectId>, to: Zone, position: Position) -> Changes {
    cards::move_cards(
        table,
        &Selection {
            objects: ids,
            zone: from,
        },
        Destination::Zone(to),
        position,
        None,
        None,
    )
}

pub(super) fn draw(table: &Table, seat: Seat, count: u16) -> Changes {
    let library = Zone::Library(seat);
    let ids = zone_cards(table, library)?;
    move_to(
        table,
        library,
        ids[..ids.len().min(usize::from(count))].to_vec(),
        Zone::Hand(seat),
        Position::Bottom,
    )
}

pub(super) fn mulligan(
    table: &Table,
    seat: Seat,
    count: u16,
    chance: &mut Chance,
    action: &Action,
) -> Result<AppliedChanges, Refusal> {
    let hand = Zone::Hand(seat);
    let library = Zone::Library(seat);
    let returned = move_to(
        table,
        hand,
        zone_cards(table, hand)?.to_vec(),
        library,
        Position::Top,
    )?;
    // The shuffle sees the returned hand and the draw sees the shuffle's new ids.
    let mut staged = table.clone();
    perform(&mut staged, &returned, action);
    let shuffled = shuffle(&staged, seat, Portion::All, chance)?;
    perform(&mut staged, &shuffled, action);
    let mut applied = AppliedChanges::default();
    applied.record(returned, None);
    applied.record(shuffled, Some(Presentation::Shuffle { library }));
    applied.record(draw(&staged, seat, count)?, None);
    Ok(applied)
}

/// Put a uniformly random sample at the front, in draw order. For a full
/// shuffle the sample is the whole slice. The last remaining card needs no roll.
fn sample(ids: &mut [ObjectId], count: usize, chance: &mut Chance) -> Result<Vec<u32>, Refusal> {
    let mut outcomes = Vec::new();
    for index in 0..count.min(ids.len().saturating_sub(1)) {
        let remaining = u32::try_from(ids.len() - index).map_err(|_| Refusal::INVALID)?;
        let offset = chance.below(remaining)?;
        ids.swap(index, index + offset as usize);
        outcomes.push(offset);
    }
    Ok(outcomes)
}

pub(super) fn plan_shuffle(
    table: &Table,
    seat: Seat,
    portion: Portion,
    chance: &mut Chance,
) -> Result<AppliedChanges, Refusal> {
    Ok(AppliedChanges::presented(
        shuffle(table, seat, portion, chance)?,
        Presentation::Shuffle {
            library: Zone::Library(seat),
        },
    ))
}

pub(super) fn shuffle(table: &Table, seat: Seat, portion: Portion, chance: &mut Chance) -> Changes {
    let zone = Zone::Library(seat);
    let ids = zone_cards(table, zone)?;
    let range = portion_range(ids.len(), portion);
    let mut shuffled = ids[range.clone()].to_vec();
    let outcomes = sample(&mut shuffled, range.len(), chance)?;
    let mut changes = vec![Change::Random { outcomes }];
    cards::remove_arrows(table, &shuffled.iter().copied().collect(), &mut changes);
    changes.extend(move_to(
        table,
        zone,
        shuffled.clone(),
        zone,
        Position::Nth(range.start),
    )?);
    for (offset, id) in shuffled.into_iter().enumerate() {
        let card = &table.objects[&id];
        let fresh = table.fresh_id(offset).ok_or(Refusal::INVALID)?;
        changes.push(Change::ResetKnowledge {
            id,
            edit: Edit {
                before: card.known_to.clone(),
                after: Vec::new(),
            },
            forget: true,
        });
        changes.push(Change::ReplaceId {
            from: id,
            to: fresh,
        });
    }
    changes.extend(
        table
            .lent_zones
            .iter()
            .enumerate()
            .rev()
            .filter_map(|(index, &(lent, to))| {
                (lent == zone).then_some(Change::Lend {
                    zone,
                    to,
                    index,
                    on: false,
                })
            }),
    );
    Ok(changes)
}

fn learn(card: &Object, seat: Seat) -> Option<Change> {
    (!card.known_to.contains(&seat)).then_some(Change::Learn { id: card.id, seat })
}

pub(super) fn look(
    table: &Table,
    by: Seat,
    seat: Seat,
    portion: Portion,
) -> Result<AppliedChanges, Refusal> {
    let library = Zone::Library(seat);
    let ids = zone_cards(table, library)?;
    let ids = ids[portion_range(ids.len(), portion)].to_vec();
    let changes = ids
        .iter()
        .filter_map(|id| learn(&table.objects[id], by))
        .collect();
    Ok(AppliedChanges::presented(
        changes,
        Presentation::Look { library, ids },
    ))
}

pub(super) fn reveal(
    table: &Table,
    zone: Zone,
    objects: Option<&[ObjectId]>,
    random: Option<u16>,
    to: &[Seat],
    chance: &mut Chance,
) -> Result<AppliedChanges, Refusal> {
    if to
        .iter()
        .any(|seat| !table.seats().any(|other| other == *seat))
    {
        return Err(Refusal::INVALID);
    }
    let mut changes = Vec::new();
    let cards = match (objects, random) {
        (Some(_), Some(_)) => return Err(Refusal::INVALID),
        (Some(ids), None) => cards::selected_cards(
            table,
            &Selection {
                zone,
                objects: ids.to_vec(),
            },
        )?,
        (None, random) => {
            // A stack marker has no card identity to reveal.
            let mut ids: Vec<_> = zone_cards(table, zone)?
                .iter()
                .copied()
                .filter(|id| table.objects.contains_key(id))
                .collect();
            if let Some(count) = random {
                let count = usize::from(count).min(ids.len());
                let outcomes = sample(&mut ids, count, chance)?;
                changes.push(Change::Random { outcomes });
                ids.truncate(count);
            }
            ids.into_iter().map(|id| &table.objects[&id]).collect()
        }
    };
    let seats: BTreeSet<_> = to.iter().copied().collect();
    for card in &cards {
        changes.extend(seats.iter().filter_map(|&seat| learn(card, seat)));
    }
    let ids = cards.iter().map(|card| card.id).collect();
    Ok(AppliedChanges::presented(
        changes,
        Presentation::Reveal {
            zone,
            ids,
            to: to.to_vec(),
        },
    ))
}

pub(super) fn lend(table: &Table, zone: Zone, to: Seat) -> Changes {
    let (Zone::Library(owner) | Zone::Hand(owner) | Zone::Sideboard(owner)) = zone else {
        return Err(Refusal::INVALID);
    };
    if to == owner || !table.seats().any(|seat| seat == to) {
        return Err(Refusal::INVALID);
    }
    zone_cards(table, zone)?;
    Ok((!table.lent_zones.contains(&(zone, to)))
        .then_some(Change::Lend {
            zone,
            to,
            index: table.lent_zones.len(),
            on: true,
        })
        .into_iter()
        .collect())
}

pub(super) fn always_reveal_top(table: &Table, seat: Seat, on: bool) -> Changes {
    top_setting(table, &table.always_reveal_top, seat, on, |index| {
        Change::AlwaysRevealTop { seat, index, on }
    })
}

pub(super) fn always_look_at_top(table: &Table, seat: Seat, on: bool) -> Changes {
    top_setting(table, &table.always_look_at_top, seat, on, |index| {
        Change::AlwaysLookAtTop { seat, index, on }
    })
}

fn top_setting(
    table: &Table,
    enabled: &[Seat],
    seat: Seat,
    on: bool,
    change: impl FnOnce(usize) -> Change,
) -> Changes {
    zone_cards(table, Zone::Library(seat))?;
    let index = match (on, enabled.iter().position(|&owner| owner == seat)) {
        (true, None) => Some(enabled.len()),
        (false, Some(index)) => Some(index),
        _ => None,
    };
    Ok(index.map(change).into_iter().collect())
}

pub(super) fn keep_top_known(table: &Table) -> Vec<Change> {
    let mut changes = Vec::new();
    for &seat in &table.always_reveal_top {
        if let Some(card) = top_card(table, seat) {
            changes.extend(table.seats().filter_map(|viewer| learn(card, viewer)));
        }
    }
    for &seat in &table.always_look_at_top {
        if !table.always_reveal_top.contains(&seat)
            && let Some(card) = top_card(table, seat)
        {
            changes.extend(learn(card, seat));
        }
    }
    changes
}

fn top_card(table: &Table, seat: Seat) -> Option<&Object> {
    let id = table.zone(Zone::Library(seat))?.first()?;
    table.objects.get(id)
}

pub(super) fn sort_hand(table: &Table, seat: Seat, by: SortBy) -> Changes {
    let zone = Zone::Hand(seat);
    let mut ids = zone_cards(table, zone)?.to_vec();
    match by {
        SortBy::Name => ids.sort_by_cached_key(|id| match &table.objects[id].card {
            card @ Card::Named(name) => card_record(card)
                .map_or_else(|| name.to_lowercase(), |record| record.name.to_lowercase()),
            Card::Custom { name, .. } => name.to_lowercase(),
            Card::Hidden => String::new(),
        }),
        SortBy::Type => ids.sort_by_cached_key(|id| match &table.objects[id].card {
            Card::Custom { types, .. } => types.to_lowercase(),
            Card::Hidden => String::new(),
            card @ Card::Named(_) => card_record(card)
                .and_then(|record| {
                    record
                        .faces
                        .first()
                        .map(|face| face.type_line.to_lowercase())
                })
                .unwrap_or_default(),
        }),
        SortBy::ManaValue => ids.sort_by_cached_key(|id| {
            card_record(&table.objects[id].card)
                .as_ref()
                .map_or(0, hand_mana_value)
        }),
    }
    cards::reorder(table, &Selection { objects: ids, zone }, Position::Top)
}

/// [CR 202.3d] says “The mana value of a split card not on the stack or of a
/// fused split spell on the stack is determined from the combined mana costs
/// of its halves.”
///
/// [CR 202.3d]: https://yawgatog.com/resources/magic-rules/#R2023d
fn hand_mana_value(record: &Record) -> u32 {
    if record.layout == "split" {
        record.faces.iter().map(facts::mana_value).sum()
    } else {
        record.faces.first().map_or(0, facts::mana_value)
    }
}
