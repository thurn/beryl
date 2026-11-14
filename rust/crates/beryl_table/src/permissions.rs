use crate::action::{Destination, SeenObject, Selection, Target};
use crate::state::{ObjectId, Zone};
use crate::{Action, Refusal, Seat, Table};

/// Checks references and access before the action can change the table.
/// For a batch, this checks every member against the supplied table.
pub fn check(table: &Table, by: Seat, action: &Action) -> Result<(), Refusal> {
    let mut references = vec![Reference::Seat(by)];
    action_references(action, &mut references);
    // Check all references first, so a stale target is reported even if
    // another card in the action is in a zone the actor cannot use.
    let mut stale = false;
    for reference in &references {
        match reference.validate(table) {
            Err(Refusal::Stale) => stale = true,
            result => result?,
        }
    }
    if stale {
        return Err(Refusal::Stale);
    }
    for reference in &references {
        if !reference.allowed(table, by) {
            return Err(Refusal::Permission);
        }
    }
    Ok(())
}

enum Reference {
    Seat(Seat),
    OwnSeat(Seat),
    Zone(Zone),
    UseZone(Zone),
    LendZone(Zone),
    Card(SeenObject),
    Peek(SeenObject),
    MoveInto { zone: Zone, id: ObjectId },
}

impl Reference {
    fn validate(&self, table: &Table) -> Result<(), Refusal> {
        match *self {
            Self::Seat(seat) | Self::OwnSeat(seat) => {
                if table.players.get(usize::from(seat.0)).is_none() {
                    return Err(Refusal::Invalid);
                }
            }
            Self::Zone(zone)
            | Self::UseZone(zone)
            | Self::LendZone(zone)
            | Self::MoveInto { zone, .. } => {
                table.zone(zone).ok_or(Refusal::Invalid)?;
            }
            Self::Card(seen) | Self::Peek(seen) => {
                table.zone(seen.zone).ok_or(Refusal::Invalid)?;
                let (zone, _) = table.position(seen.id).ok_or(Refusal::Invalid)?;
                if zone != seen.zone {
                    return Err(Refusal::Stale);
                }
            }
        }
        Ok(())
    }

    fn allowed(&self, table: &Table, by: Seat) -> bool {
        match *self {
            Self::Seat(_) | Self::Zone(_) => true,
            Self::OwnSeat(seat) => seat == by,
            Self::UseZone(zone) => can_use_zone(table, by, zone),
            Self::LendZone(zone) => hidden_owner(zone).is_none_or(|owner| owner == by),
            Self::Card(seen) => can_use_card(table, by, seen),
            Self::Peek(seen) => {
                can_use_card(table, by, seen)
                    && table
                        .objects
                        .get(&seen.id)
                        .is_some_and(|card| card.controller == by)
            }
            Self::MoveInto { zone, id } => {
                // Either player can return a public or otherwise accessible
                // card to its owner's pile, for example when a spell bounces it.
                can_use_zone(table, by, zone)
                    || table
                        .objects
                        .get(&id)
                        .is_some_and(|card| hidden_owner(zone) == Some(card.owner))
            }
        }
    }
}

fn hidden_owner(zone: Zone) -> Option<Seat> {
    match zone {
        Zone::Hand(seat) | Zone::Library(seat) | Zone::Sideboard(seat) => Some(seat),
        Zone::Battlefield | Zone::Stack | Zone::Graveyard(_) | Zone::Exile(_) => None,
    }
}

fn can_use_zone(table: &Table, by: Seat, zone: Zone) -> bool {
    hidden_owner(zone).is_none_or(|owner| owner == by) || table.lent_zones.contains(&(zone, by))
}

fn can_use_card(table: &Table, by: Seat, seen: SeenObject) -> bool {
    can_use_zone(table, by, seen.zone)
        || (matches!(seen.zone, Zone::Hand(_))
            && table
                .objects
                .get(&seen.id)
                .is_some_and(|card| card.known_to.contains(&by)))
}

fn selection_references(
    references: &mut Vec<Reference>,
    selection: &Selection,
    reference: fn(SeenObject) -> Reference,
) {
    references.push(Reference::Zone(selection.zone));
    references.extend(selection.objects.iter().map(|&id| {
        reference(SeenObject {
            id,
            zone: selection.zone,
        })
    }));
}

fn target_reference(target: Target) -> Reference {
    match target {
        Target::Object(seen) => Reference::Card(seen),
        Target::Player(seat) => Reference::Seat(seat),
    }
}

fn action_references(action: &Action, references: &mut Vec<Reference>) {
    match action {
        Action::Move { selection, to, .. } => {
            selection_references(references, selection, Reference::Card);
            if let Destination::Zone(zone) = *to {
                references.push(Reference::Zone(zone));
                if zone != selection.zone {
                    references.extend(
                        selection
                            .objects
                            .iter()
                            .map(|&id| Reference::MoveInto { zone, id }),
                    );
                }
            }
        }
        Action::Reorder { selection, .. }
        | Action::Pin { selection, .. }
        | Action::Tap { selection, .. }
        | Action::Face { selection, .. }
        | Action::Flag { selection, .. }
        | Action::Counter { selection, .. }
        | Action::Stats { selection, .. }
        | Action::Damage { selection, .. }
        | Action::Note { selection, .. }
        | Action::Unattach { selection }
        | Action::Clone { selection }
        | Action::Marker { selection, .. }
        | Action::Remove { selection } => {
            selection_references(references, selection, Reference::Card);
        }
        Action::Peek { selection } => selection_references(references, selection, Reference::Peek),
        Action::Give { selection, to } => {
            selection_references(references, selection, Reference::Card);
            references.push(Reference::Seat(*to));
        }
        Action::Attach { selection, host } => {
            selection_references(references, selection, Reference::Card);
            references.push(Reference::Card(*host));
        }
        Action::Token { attached_to, .. } => {
            references.extend(attached_to.map(Reference::Card));
        }
        Action::Arrow { selection, to } => {
            selection_references(references, selection, Reference::Card);
            references.push(target_reference(*to));
        }
        Action::ClearArrows { from, to } => {
            references.extend(from.map(Reference::Card));
            references.extend(to.map(target_reference));
        }
        Action::Draw { seat, .. } | Action::Mulligan { seat, .. } => {
            references.push(Reference::UseZone(Zone::Library(*seat)));
            references.push(Reference::UseZone(Zone::Hand(*seat)));
        }
        Action::Shuffle { seat, .. } | Action::Look { seat, .. } | Action::Flow { seat, .. } => {
            references.push(Reference::UseZone(Zone::Library(*seat)));
        }
        Action::SortHand { seat, .. } => references.push(Reference::UseZone(Zone::Hand(*seat))),
        Action::Reveal {
            zone, objects, to, ..
        } => {
            if let Some(objects) = objects {
                references.push(Reference::Zone(*zone));
                references.extend(
                    objects
                        .iter()
                        .map(|&id| Reference::Card(SeenObject { id, zone: *zone })),
                );
            } else {
                references.push(Reference::UseZone(*zone));
            }
            references.extend(to.iter().copied().map(Reference::Seat));
        }
        Action::Lend { zone, to } => {
            references.push(Reference::LendZone(*zone));
            references.push(Reference::Seat(*to));
        }
        Action::AlwaysRevealTop { seat, .. }
        | Action::AlwaysLookAtTop { seat, .. }
        | Action::LoadDeck { seat, .. }
        | Action::Sideboard { seat, .. }
        | Action::Ready { seat, .. } => references.push(Reference::OwnSeat(*seat)),
        Action::Life { seat, .. } | Action::PlayerCounter { seat, .. } => {
            references.push(Reference::Seat(*seat));
        }
        Action::Start { first } => references.push(Reference::Seat(*first)),
        Action::NextGame { winner } => references.extend(winner.map(Reference::Seat)),
        Action::Batch { actions } => {
            for action in actions {
                action_references(action, references);
            }
        }
        // Concession, assists and the undo key refer to the acting seat.
        // Which log entries may be undone depends on the log, not the table.
        Action::Phase { .. }
        | Action::NextTurn {}
        | Action::Roll { .. }
        | Action::Concede {}
        | Action::SetAssist { .. }
        | Action::Undo {}
        | Action::Rewind { .. } => {}
    }
}
