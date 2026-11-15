use crate::action::{Destination, SeenObject, Selection, Target};
use crate::state::{ObjectId, Zone};
use crate::{Action, Refusal, Seat, Table};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RevealRules {
    Explicit,
    /// Historical save entries used knowledge as permission for hand cards.
    Legacy,
}

/// Checks references and access before the action can change the table.
/// For a batch, this checks every member against the supplied table.
pub fn check(table: &Table, by: Seat, action: &Action) -> Result<(), Refusal> {
    check_with_reveals(table, by, action, RevealRules::Explicit)
}

pub(crate) fn check_with_reveals(
    table: &Table,
    by: Seat,
    action: &Action,
    reveals: RevealRules,
) -> Result<(), Refusal> {
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
        if !reference.allowed(table, by, reveals) {
            return Err(Refusal::Permission);
        }
    }
    Ok(())
}

pub(crate) fn check_actor(table: &Table, by: Seat) -> Result<(), Refusal> {
    Reference::Seat(by).validate(table)
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
                    return Err(Refusal::INVALID);
                }
            }
            Self::Zone(zone)
            | Self::UseZone(zone)
            | Self::LendZone(zone)
            | Self::MoveInto { zone, .. } => {
                table.zone(zone).ok_or(Refusal::INVALID)?;
            }
            Self::Card(seen) | Self::Peek(seen) => {
                table.zone(seen.zone).ok_or(Refusal::INVALID)?;
                let (zone, _) = table.position(seen.id).ok_or(Refusal::INVALID)?;
                if zone != seen.zone {
                    return Err(Refusal::Stale);
                }
            }
        }
        Ok(())
    }

    fn allowed(&self, table: &Table, by: Seat, reveals: RevealRules) -> bool {
        match *self {
            Self::Seat(_) | Self::Zone(_) => true,
            Self::OwnSeat(seat) => seat == by,
            Self::UseZone(zone) => can_use_zone(table, by, zone),
            Self::LendZone(zone) => zone.hidden_owner().is_none_or(|owner| owner == by),
            Self::Card(seen) => can_use_card(table, by, seen, reveals),
            Self::Peek(seen) => {
                can_use_card(table, by, seen, reveals)
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
                        .is_some_and(|card| zone.hidden_owner() == Some(card.owner))
            }
        }
    }
}

fn can_use_zone(table: &Table, by: Seat, zone: Zone) -> bool {
    zone.hidden_owner().is_none_or(|owner| owner == by) || table.lent_zones.contains(&(zone, by))
}

/// The expected zone to offer with a projected object the seat can act on.
pub fn reference_zone(table: &Table, by: Seat, id: ObjectId) -> Option<Zone> {
    table.players.get(usize::from(by.0))?;
    let (zone, _) = table.position(id)?;
    can_use_card(table, by, SeenObject { id, zone }, RevealRules::Explicit).then_some(zone)
}

fn can_use_card(table: &Table, by: Seat, seen: SeenObject, reveals: RevealRules) -> bool {
    if can_use_zone(table, by, seen.zone) {
        return true;
    }
    if reveals == RevealRules::Legacy {
        return matches!(seen.zone, Zone::Hand(_))
            && table
                .objects
                .get(&seen.id)
                .is_some_and(|card| card.known_to.contains(&by));
    }
    table
        .objects
        .get(&seen.id)
        .is_some_and(|card| card.revealed_to.contains(&by))
        || matches!(seen.zone, Zone::Library(owner)
            if table.always_reveal_top.contains(&owner)
                && table.zone(seen.zone).and_then(|ids| ids.first()) == Some(&seen.id))
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
        Action::Token { attached_to, .. } => references.extend(attached_to.map(Reference::Card)),
        Action::Arrow { selection, to } => {
            selection_references(references, selection, Reference::Card);
            references.push(target_reference(*to));
        }
        Action::ClearArrows { from, to } => {
            references.extend(from.map(Reference::Card));
            references.extend(to.map(target_reference));
        }
        Action::Draw { seat, .. } | Action::Mulligan { seat, .. } => references.extend([
            Reference::UseZone(Zone::Library(*seat)),
            Reference::UseZone(Zone::Hand(*seat)),
        ]),
        Action::Shuffle { seat, .. }
        | Action::Look { seat, .. }
        | Action::StackUntil { seat, .. }
        | Action::Flow { seat, .. } => references.push(Reference::UseZone(Zone::Library(*seat))),
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
        Action::Life { seat, .. }
        | Action::PlayerCounter { seat, .. }
        | Action::Start { first: seat } => references.push(Reference::Seat(*seat)),
        Action::NextGame { winner } => references.extend(winner.map(Reference::Seat)),
        Action::Batch { actions } => {
            for action in actions {
                action_references(action, references);
            }
        }
        // Concession, assists and the undo key refer to the acting seat.
        // Which log entries may be undone depends on the log, not the table.
        Action::CloseLook {}
        | Action::Phase { .. }
        | Action::NextTurn {}
        | Action::Roll { .. }
        | Action::Concede {}
        | Action::SetAssist { .. }
        | Action::Undo { .. }
        | Action::Rewind { .. } => {}
    }
}
