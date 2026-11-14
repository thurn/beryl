use std::collections::{BTreeMap, BTreeSet};

use beryl_card_store::{Record, STORE, Side, facts};

use crate::action::{
    Adjustment, ComputedDestination, Destination, Position, SeenObject, Selection,
};
use crate::change::{Edit, Location, ObjectEdit};
use crate::state::{AbilityMarker, ArrowTarget, Card, Face, Object, ObjectId, Row, Zone};
use crate::{Change, Refusal, Seat, Table};

use super::{Changes, edit, players};

pub(super) fn move_cards(
    table: &Table,
    selection: &Selection,
    destination: Destination,
    position: Position,
    face: Option<Face>,
    tapped: Option<bool>,
) -> Changes {
    let selected = selected_ids(table, selection)?;
    let mut source = table.zone(selection.zone).expect("validated zone").to_vec();
    let moves = plan_moves(table, selection, destination, face, tapped)?;
    let mut destinations =
        destination_positions(table, selection.zone, &selected, &moves, position)?;
    if moves.is_empty()
        && let Destination::Zone(to) = destination
    {
        insertion_index(position, table.zone(to).ok_or(Refusal::Invalid)?.len())?;
    }

    let leaving: BTreeSet<_> = moves
        .iter()
        .filter_map(|(id, target)| {
            target
                .as_ref()
                .is_none_or(|target| target.zone != selection.zone)
                .then_some(*id)
        })
        .collect();
    let mut changes = Vec::new();
    remove_arrows(table, &leaving, &mut changes);
    if selection.zone == Zone::Battlefield {
        detach_cards(table, &leaving, &mut changes);
    }

    // Insert in reverse selection order so cards keep their order even when
    // they go to different destinations. All indices describe the running state.
    let mut next_id = 0;
    for (id, target) in moves.into_iter().rev() {
        let index = source_index(&source, id);
        source.remove(index);
        let Some(Arrival {
            zone: to,
            face,
            tapped,
            counters,
        }) = target
        else {
            changes.push(removal(table, id, index));
            continue;
        };
        let card = &table.objects[&id];
        if card.zone == Zone::Battlefield
            && card.token
            && !matches!(to, Zone::Battlefield | Zone::Stack)
        {
            changes.push(removal(table, id, index));
            continue;
        }
        let Insertion {
            index: target_index,
            anchor,
        } = destinations.get_mut(&to).expect("planned destination");
        let insert = if selection.zone == to {
            let index = anchor.map_or(source.len(), |anchor| source_index(&source, anchor));
            source.insert(index, id);
            *anchor = Some(id);
            index
        } else {
            *target_index
        };
        if selection.zone != to || index != insert {
            changes.push(Change::Move {
                id,
                from: Location {
                    zone: selection.zone,
                    index,
                },
                to: Location {
                    zone: to,
                    index: insert,
                },
            });
        }
        update_card(table, card, to, face, tapped, &mut changes);
        if let Some((name, count)) = counters {
            changes.extend(
                object_counter(card, name, Adjustment::Set(i32::from(count)))
                    .map(|edit| Change::SetObject { id, edit }),
            );
        }
        if let Zone::Hand(owner) = to
            && selection.zone != to
        {
            let fresh = table.fresh_id(next_id).ok_or(Refusal::Invalid)?;
            next_id += 1;
            changes.push(Change::ResetKnowledge {
                id,
                edit: Edit {
                    before: card.known_to.clone(),
                    after: vec![owner],
                },
                forget: true,
            });
            changes.push(Change::ReplaceId {
                from: id,
                to: fresh,
            });
        }
    }
    Ok(changes)
}

struct Arrival {
    zone: Zone,
    face: Face,
    tapped: Option<bool>,
    counters: Option<(&'static str, u16)>,
}

fn plan_moves(
    table: &Table,
    selection: &Selection,
    destination: Destination,
    face: Option<Face>,
    tapped: Option<bool>,
) -> Result<Vec<(ObjectId, Option<Arrival>)>, Refusal> {
    let mut moves = Vec::with_capacity(selection.objects.len());
    for &id in &selection.objects {
        let Some(card) = table.objects.get(&id) else {
            if destination != Destination::Computed(ComputedDestination::Resolve) {
                return Err(Refusal::Invalid);
            }
            moves.push((id, None));
            continue;
        };
        let face = face.unwrap_or(card.face);
        let needs_printed_face = match destination {
            Destination::Zone(zone) => zone == Zone::Battlefield && card.zone != zone,
            Destination::Computed(_) => true,
        };
        let record = (needs_printed_face && face != Face::Down)
            .then(|| card_record(&card.card))
            .flatten();
        let printed = record
            .as_ref()
            .and_then(|record| printed_face(record, face));
        let zone = destination_zone(card, destination, face, printed)?;
        let enters = card.zone != Zone::Battlefield && zone == Zone::Battlefield;
        let tapped = tapped.or_else(|| {
            (enters && matches!(destination, Destination::Computed(_)))
                .then(|| printed.is_some_and(facts::enters_tapped))
        });
        let counters = printed
            .filter(|_| enters)
            .and_then(facts::starting_counters);
        moves.push((
            id,
            Some(Arrival {
                zone,
                face,
                tapped,
                counters,
            }),
        ));
    }
    Ok(moves)
}

struct Insertion {
    index: usize,
    anchor: Option<ObjectId>,
}

fn destination_positions(
    table: &Table,
    source: Zone,
    selected: &BTreeSet<ObjectId>,
    moves: &[(ObjectId, Option<Arrival>)],
    position: Position,
) -> Result<BTreeMap<Zone, Insertion>, Refusal> {
    let zones: BTreeSet<_> = moves
        .iter()
        .filter_map(|(_, arrival)| Some(arrival.as_ref()?.zone))
        .collect();
    zones
        .into_iter()
        .map(|zone| {
            let target = table.zone(zone).ok_or(Refusal::Invalid)?;
            let same_zone = source == zone;
            let target_len = target.len() - if same_zone { selected.len() } else { 0 };
            let index = insertion_index(position, target_len)?;
            let anchor = if same_zone {
                target
                    .iter()
                    .copied()
                    .filter(|id| !selected.contains(id))
                    .nth(index)
            } else {
                None
            };
            Ok((zone, Insertion { index, anchor }))
        })
        .collect()
}

fn destination_zone(
    card: &Object,
    destination: Destination,
    face: Face,
    printed: Option<&beryl_card_store::Face>,
) -> Result<Zone, Refusal> {
    let type_line = match &card.card {
        Card::Custom { types, .. } => types,
        Card::Named(_) => printed.map_or("", |printed| printed.type_line.as_str()),
        Card::Hidden => "",
    };
    match destination {
        Destination::Zone(to) => Ok(to),
        Destination::Computed(ComputedDestination::Play) => {
            if face == Face::Down || facts::is_land_type_line(type_line) {
                Ok(Zone::Battlefield)
            } else {
                Ok(Zone::Stack)
            }
        }
        Destination::Computed(ComputedDestination::Resolve) if card.zone == Zone::Stack => {
            if face == Face::Down || facts::is_permanent_type_line(type_line) {
                Ok(Zone::Battlefield)
            } else {
                Ok(Zone::Graveyard(card.owner))
            }
        }
        Destination::Computed(ComputedDestination::Resolve) => Err(Refusal::Invalid),
    }
}

pub(super) fn card_record(card: &Card) -> Option<Record> {
    match card {
        Card::Named(name) => STORE
            .card(name)
            .or_else(|| STORE.tokens(name).into_iter().next()),
        Card::Hidden | Card::Custom { .. } => None,
    }
}

pub(super) fn printed_face(record: &Record, face: Face) -> Option<&beryl_card_store::Face> {
    match face {
        Face::Front => record.faces.first(),
        Face::Back => record.faces.iter().find(|face| face.side == Side::Back),
        Face::Down => None,
    }
}

fn insertion_index(position: Position, len: usize) -> Result<usize, Refusal> {
    match position {
        Position::Top => Ok(0),
        Position::Bottom => Ok(len),
        Position::Nth(index) if index <= len => Ok(index),
        Position::Nth(_) => Err(Refusal::Invalid),
    }
}

fn source_index(source: &[ObjectId], id: ObjectId) -> usize {
    source
        .iter()
        .position(|&candidate| candidate == id)
        .expect("validated source")
}

pub(super) fn remove_arrows(
    table: &Table,
    selected: &BTreeSet<ObjectId>,
    changes: &mut Vec<Change>,
) {
    changes.extend(players::remove_arrows(table, |arrow| {
        selected.contains(&arrow.from)
            || matches!(arrow.to, ArrowTarget::Object(id) if selected.contains(&id))
    }));
}

fn detach_cards(table: &Table, selected: &BTreeSet<ObjectId>, changes: &mut Vec<Change>) {
    for card in table.objects.values() {
        if let Some(host) = card.attached_to
            && (selected.contains(&card.id) || selected.contains(&host))
        {
            changes.push(Change::SetObject {
                id: card.id,
                edit: ObjectEdit::AttachedTo(Edit {
                    before: Some(host),
                    after: None,
                }),
            });
        }
    }
}

fn update_card(
    table: &Table,
    card: &Object,
    to: Zone,
    face: Face,
    tapped: Option<bool>,
    changes: &mut Vec<Change>,
) {
    let mut set = |edit| changes.push(Change::SetObject { id: card.id, edit });
    if face != card.face {
        set(ObjectEdit::Face(Edit {
            before: card.face,
            after: face,
        }));
    }
    let leaves_battlefield = card.zone == Zone::Battlefield && to != Zone::Battlefield;
    let tapped = if leaves_battlefield {
        false
    } else {
        tapped.unwrap_or(card.tapped)
    };
    if tapped != card.tapped {
        set(ObjectEdit::Tapped(Edit {
            before: card.tapped,
            after: tapped,
        }));
    }
    if leaves_battlefield {
        if !card.counters.is_empty() {
            set(ObjectEdit::Counters(Edit {
                before: card.counters.clone(),
                after: Vec::new(),
            }));
        }
        if card.stats.is_some() {
            set(ObjectEdit::Stats(Edit {
                before: card.stats,
                after: None,
            }));
        }
        if card.damage != 0 {
            set(ObjectEdit::Damage(Edit {
                before: card.damage,
                after: 0,
            }));
        }
        if card.doesnt_untap {
            set(ObjectEdit::DoesntUntap(Edit {
                before: true,
                after: false,
            }));
        }
        if card.note.is_some() && to != Zone::Stack {
            set(ObjectEdit::Note(Edit {
                before: card.note.clone(),
                after: None,
            }));
        }
        if card.row != Row::Auto {
            set(ObjectEdit::Row(Edit {
                before: card.row,
                after: Row::Auto,
            }));
        }
        if card.controller != card.owner {
            set(ObjectEdit::Controller(Edit {
                before: card.controller,
                after: card.owner,
            }));
        }
    }
    if to.is_public() && face != Face::Down {
        changes.extend(reveal(table, card));
    }
    if let Zone::Sideboard(owner) = to
        && !card.known_to.contains(&owner)
    {
        changes.push(Change::Learn {
            id: card.id,
            seat: owner,
        });
    }
}

fn selected_ids(table: &Table, selection: &Selection) -> Result<BTreeSet<ObjectId>, Refusal> {
    let mut selected = BTreeSet::new();
    for &id in &selection.objects {
        validate_seen(
            table,
            SeenObject {
                id,
                zone: selection.zone,
            },
        )?;
        if !selected.insert(id) {
            return Err(Refusal::Invalid);
        }
    }
    table.zone(selection.zone).ok_or(Refusal::Invalid)?;
    Ok(selected)
}

pub(super) fn validate_seen(table: &Table, seen: SeenObject) -> Result<(), Refusal> {
    let (zone, _) = table.position(seen.id).ok_or(Refusal::Invalid)?;
    if zone != seen.zone {
        return Err(Refusal::Stale);
    }
    Ok(())
}

pub(super) fn selected_cards<'a>(
    table: &'a Table,
    selection: &Selection,
) -> Result<Vec<&'a Object>, Refusal> {
    selected_ids(table, selection)?;
    selection
        .objects
        .iter()
        .map(|id| table.objects.get(id).ok_or(Refusal::Invalid))
        .collect()
}

fn set_cards(
    table: &Table,
    selection: &Selection,
    edit: impl Fn(&Object) -> Option<ObjectEdit>,
) -> Changes {
    Ok(selected_cards(table, selection)?
        .into_iter()
        .filter_map(|card| {
            Some(Change::SetObject {
                id: card.id,
                edit: edit(card)?,
            })
        })
        .collect())
}

fn reveal<'a>(table: &'a Table, card: &'a Object) -> impl Iterator<Item = Change> + 'a {
    table
        .seats()
        .filter(|seat| !card.known_to.contains(seat))
        .map(|seat| Change::Learn { id: card.id, seat })
}

fn battlefield(selection: &Selection) -> Result<(), Refusal> {
    if selection.zone != Zone::Battlefield {
        return Err(Refusal::Invalid);
    }
    Ok(())
}

pub(super) fn tap(table: &Table, selection: &Selection, tapped: bool) -> Changes {
    battlefield(selection)?;
    set_cards(table, selection, |card| {
        edit(card.tapped, tapped, ObjectEdit::Tapped)
    })
}

pub(super) fn reorder(table: &Table, selection: &Selection, position: Position) -> Changes {
    if !matches!(selection.zone, Zone::Battlefield | Zone::Hand(_)) {
        return Err(Refusal::Invalid);
    }
    move_cards(
        table,
        selection,
        Destination::Zone(selection.zone),
        position,
        None,
        None,
    )
}

pub(super) fn pin(table: &Table, selection: &Selection, row: Row) -> Changes {
    battlefield(selection)?;
    set_cards(table, selection, |card| {
        edit(card.row, row, ObjectEdit::Row)
    })
}

pub(super) fn give(table: &Table, selection: &Selection, to: Seat) -> Changes {
    battlefield(selection)?;
    if table.players.len() < 2 || usize::from(to.0) >= table.players.len() {
        return Err(Refusal::Invalid);
    }
    set_cards(table, selection, |card| {
        edit(card.controller, to, ObjectEdit::Controller)
    })
}

pub(super) fn face(table: &Table, selection: &Selection, face: Face) -> Changes {
    let mut changes = Vec::new();
    for card in selected_cards(table, selection)? {
        if let Some(edit) = edit(card.face, face, ObjectEdit::Face) {
            changes.push(Change::SetObject { id: card.id, edit });
        }
        if card.zone.is_public() && face != Face::Down {
            changes.extend(reveal(table, card));
        }
    }
    Ok(changes)
}

pub(super) fn flag(table: &Table, selection: &Selection, doesnt_untap: bool) -> Changes {
    battlefield(selection)?;
    set_cards(table, selection, |card| {
        edit(card.doesnt_untap, doesnt_untap, ObjectEdit::DoesntUntap)
    })
}

pub(super) fn peek(table: &Table, by: Seat, selection: &Selection) -> Changes {
    let mut changes = Vec::new();
    for card in selected_cards(table, selection)? {
        if card.face != Face::Down {
            return Err(Refusal::Invalid);
        }
        if !card.known_to.contains(&by) {
            changes.push(Change::Learn {
                id: card.id,
                seat: by,
            });
        }
    }
    Ok(changes)
}

pub(super) fn counter_edit(
    counters: &[(String, i32)],
    name: &str,
    adjustment: Adjustment,
    maximum: i32,
) -> Option<Edit<Vec<(String, i32)>>> {
    let index = counters.iter().position(|(existing, _)| existing == name);
    let current = index.map_or(0, |index| counters[index].1);
    let count = adjustment.apply(current).clamp(0, maximum);
    if current == count && (count != 0 || index.is_none()) {
        return None;
    }
    let mut after = counters.to_vec();
    match index {
        Some(index) if count == 0 => {
            after.remove(index);
        }
        Some(index) => after[index].1 = count,
        None => after.push((name.to_owned(), count)),
    }
    Some(Edit {
        before: counters.to_vec(),
        after,
    })
}

fn object_counter(card: &Object, name: &str, adjustment: Adjustment) -> Option<ObjectEdit> {
    counter_edit(&card.counters, name, adjustment, 999).map(ObjectEdit::Counters)
}

pub(super) fn counter(
    table: &Table,
    selection: &Selection,
    name: &str,
    adjustment: Adjustment,
) -> Changes {
    set_cards(table, selection, |card| {
        object_counter(card, name, adjustment)
    })
}

pub(super) fn stats(table: &Table, selection: &Selection, stats: Option<(i32, i32)>) -> Changes {
    battlefield(selection)?;
    set_cards(table, selection, |card| {
        edit(card.stats, stats, ObjectEdit::Stats)
    })
}

pub(super) fn damage(table: &Table, selection: &Selection, damage: i32) -> Changes {
    battlefield(selection)?;
    if damage < 0 {
        return Err(Refusal::Invalid);
    }
    set_cards(table, selection, |card| {
        edit(card.damage, damage, ObjectEdit::Damage)
    })
}

pub(super) fn note(table: &Table, selection: &Selection, note: Option<&str>) -> Changes {
    set_cards(table, selection, |card| {
        edit(card.note.clone(), note.map(str::to_owned), ObjectEdit::Note)
    })
}

fn attachment_host(table: &Table, host: SeenObject) -> Result<ObjectId, Refusal> {
    validate_seen(table, host)?;
    if host.zone != Zone::Battlefield {
        return Err(Refusal::Invalid);
    }
    Ok(host.id)
}

pub(super) fn attach(table: &Table, selection: &Selection, host: SeenObject) -> Changes {
    let host = attachment_host(table, host)?;
    let cards = selected_cards(table, selection)?;
    let mut ancestor = Some(host);
    let mut visited = BTreeSet::new();
    while let Some(id) = ancestor {
        if selection.objects.contains(&id) || !visited.insert(id) {
            return Err(Refusal::Invalid);
        }
        ancestor = table.objects.get(&id).and_then(|card| card.attached_to);
    }
    let mut changes = if selection.zone == Zone::Battlefield {
        Vec::new()
    } else {
        move_cards(
            table,
            selection,
            Destination::Zone(Zone::Battlefield),
            Position::Top,
            None,
            None,
        )?
    };
    changes.extend(cards.into_iter().filter_map(|card| {
        Some(Change::SetObject {
            id: card.id,
            edit: edit(card.attached_to, Some(host), ObjectEdit::AttachedTo)?,
        })
    }));
    Ok(changes)
}

pub(super) fn unattach(table: &Table, selection: &Selection) -> Changes {
    battlefield(selection)?;
    set_cards(table, selection, |card| {
        edit(card.attached_to, None, ObjectEdit::AttachedTo)
    })
}

fn create_token(
    table: &Table,
    mut object: Object,
    index: usize,
    record: Option<&Record>,
) -> Change {
    object.token = true;
    if object.face != Face::Down {
        object.known_to = table.seats().collect();
    }
    if let Some((name, count)) = record
        .and_then(|record| printed_face(record, object.face))
        .and_then(facts::starting_counters)
        && count != 0
    {
        object
            .counters
            .push((name.to_owned(), i32::from(count).min(999)));
    }
    Change::Create { object, index }
}

pub(super) fn clone(table: &Table, by: Seat, selection: &Selection) -> Changes {
    selected_cards(table, selection)?
        .into_iter()
        .enumerate()
        .map(|(index, card)| {
            let id = table.fresh_id(index).ok_or(Refusal::Invalid)?;
            let mut copy = Object::new(id, card.card.clone(), by, Zone::Battlefield);
            copy.face = card.face;
            copy.known_to.clone_from(&card.known_to);
            let record = card_record(&copy.card);
            Ok(create_token(table, copy, index, record.as_ref()))
        })
        .collect()
}

pub(super) fn token(
    table: &Table,
    by: Seat,
    card: &Card,
    count: u16,
    host: Option<SeenObject>,
    note: Option<&str>,
) -> Changes {
    let attached_to = host.map(|host| attachment_host(table, host)).transpose()?;
    let record = match card {
        Card::Hidden => return Err(Refusal::Invalid),
        Card::Named(name) => Some(
            STORE
                .tokens(name)
                .into_iter()
                .next()
                .ok_or(Refusal::Invalid)?,
        ),
        Card::Custom { .. } => None,
    };
    (0..usize::from(count))
        .map(|index| {
            let id = table.fresh_id(index).ok_or(Refusal::Invalid)?;
            let mut token = Object::new(id, card.clone(), by, Zone::Battlefield);
            token.attached_to = attached_to;
            token.note = note.map(str::to_owned);
            Ok(create_token(table, token, index, record.as_ref()))
        })
        .collect()
}

pub(super) fn marker(table: &Table, selection: &Selection, text: &str) -> Changes {
    selected_cards(table, selection)?
        .into_iter()
        .enumerate()
        .map(|(index, card)| {
            let id = table.fresh_id(index).ok_or(Refusal::Invalid)?;
            let marker = AbilityMarker {
                source: card.id,
                text: text.to_owned(),
            };
            Ok(Change::CreateMarker { id, marker, index })
        })
        .collect()
}

fn removal(table: &Table, id: ObjectId, index: usize) -> Change {
    if let Some(marker) = table.markers.get(&id) {
        Change::RemoveMarker {
            id,
            marker: marker.clone(),
            index,
        }
    } else {
        let mut object = table.objects[&id].clone();
        if object.zone == Zone::Battlefield {
            // The preceding detach is undone separately.
            object.attached_to = None;
        }
        Change::Remove { object, index }
    }
}

pub(super) fn remove(table: &Table, selection: &Selection) -> Changes {
    let selected = selected_ids(table, selection)?;
    let source = table.zone(selection.zone).expect("validated zone");
    if selected
        .iter()
        .any(|id| table.objects.get(id).is_some_and(|card| !card.token))
    {
        return Err(Refusal::Invalid);
    }
    let mut changes = Vec::new();
    remove_arrows(table, &selected, &mut changes);
    if selection.zone == Zone::Battlefield {
        detach_cards(table, &selected, &mut changes);
    }
    changes.extend(
        source
            .iter()
            .copied()
            .enumerate()
            .rev()
            .filter(|(_, id)| selected.contains(id))
            .map(|(index, id)| removal(table, id, index)),
    );
    Ok(changes)
}
