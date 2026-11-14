use std::collections::BTreeSet;

use crate::action::{Destination, Position, Selection};
use crate::change::{Edit, Location, ObjectEdit};
use crate::state::{ArrowTarget, Face, Object, ObjectId, Row, Zone};
use crate::{Action, Change, Refusal, Table};

use super::Changes;

pub(super) fn move_cards(
    table: &Table,
    selection: &Selection,
    destination: Destination,
    position: Position,
    face: Option<Face>,
    tapped: Option<bool>,
) -> Changes {
    let Destination::Zone(to) = destination else {
        return Err(Refusal::Invalid);
    };
    let selected = selected_cards(table, selection)?;
    let mut source = table.zone(selection.zone).ok_or(Refusal::Invalid)?.to_vec();
    let target = table.zone(to).ok_or(Refusal::Invalid)?;
    let same_zone = selection.zone == to;
    let leaves_battlefield = selection.zone == Zone::Battlefield && !same_zone;

    let target_len = if same_zone {
        target.len() - selected.len()
    } else {
        target.len()
    };
    let target_index = match position {
        Position::Top => 0,
        Position::Bottom => target_len,
        Position::Nth(index) if index <= target_len => index,
        Position::Nth(_) => return Err(Refusal::Invalid),
    };
    let mut anchor = None;
    if same_zone {
        anchor = target
            .iter()
            .copied()
            .filter(|id| !selected.contains(id))
            .nth(target_index);
    }

    let mut changes = Vec::new();
    if !same_zone {
        remove_arrows(table, &selected, &mut changes);
    }
    if leaves_battlefield {
        detach_cards(table, &selected, &mut changes);
    }

    // Insert from the last selected card to the first. Each card then either
    // precedes the next selected card or goes at the requested position.
    let mut next_id = 0;
    for &id in selection.objects.iter().rev() {
        let card = &table.objects[&id];
        let index = source
            .iter()
            .position(|&candidate| candidate == id)
            .expect("validated source");
        source.remove(index);
        if leaves_battlefield && card.token && to != Zone::Stack {
            let mut object = card.clone();
            object.attached_to = None; // The preceding detach is undone separately.
            changes.push(Change::Remove { object, index });
            continue;
        }
        let insert = if same_zone {
            let index = anchor.map_or(source.len(), |anchor| {
                source
                    .iter()
                    .position(|&id| id == anchor)
                    .expect("destination anchor")
            });
            source.insert(index, id);
            anchor = Some(id);
            index
        } else {
            target_index
        };
        if !same_zone || index != insert {
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
        if let Zone::Hand(owner) = to
            && !same_zone
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

fn remove_arrows(table: &Table, selected: &BTreeSet<ObjectId>, changes: &mut Vec<Change>) {
    for (index, arrow) in table.arrows.iter().enumerate().rev() {
        let target_moved = matches!(arrow.to, ArrowTarget::Object(id) if selected.contains(&id));
        if selected.contains(&arrow.from) || target_moved {
            changes.push(Change::RemoveArrow {
                arrow: arrow.clone(),
                index,
            });
        }
    }
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
    face: Option<Face>,
    tapped: Option<bool>,
    changes: &mut Vec<Change>,
) {
    let mut set = |edit| changes.push(Change::SetObject { id: card.id, edit });
    let face = face.unwrap_or(card.face);
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
        changes.extend(
            table
                .seats()
                .filter(|seat| !card.known_to.contains(seat))
                .map(|seat| Change::Learn { id: card.id, seat }),
        );
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

fn selected_cards(table: &Table, selection: &Selection) -> Result<BTreeSet<ObjectId>, Refusal> {
    let mut selected = BTreeSet::new();
    for &id in &selection.objects {
        let (zone, _) = table.position(id).ok_or(Refusal::Invalid)?;
        if zone != selection.zone {
            return Err(Refusal::Stale);
        }
        if !table.objects.contains_key(&id) || !selected.insert(id) {
            return Err(Refusal::Invalid);
        }
    }
    Ok(selected)
}

pub(super) fn tap(table: &Table, selection: &Selection, tapped: bool) -> Changes {
    if selection.zone != Zone::Battlefield {
        return Err(Refusal::Invalid);
    }
    selected_cards(table, selection)?;
    Ok(selection
        .objects
        .iter()
        .filter_map(|&id| {
            let before = table.objects[&id].tapped;
            (before != tapped).then_some(Change::SetObject {
                id,
                edit: ObjectEdit::Tapped(Edit {
                    before,
                    after: tapped,
                }),
            })
        })
        .collect())
}

super::unimplemented_actions!(
    reorder, pin, give, face, flag, peek, counter, stats, damage, note, attach, unattach, clone,
    token, marker, remove,
);
