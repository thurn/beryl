//! The ordered presentation of an entry for one viewer.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::apply::Presentation;
use crate::change::Location;
use crate::state::{Card, Face, ObjectId, Phase, Zone};
use crate::{Change, Entry, Seat, Table};

use super::{HiddenNames, forgotten_ids, project_change, redact_projected_card, visit_members};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AnimationProjection {
    pub n: u32,
    pub by: Seat,
    pub members: Vec<AnimationMember>,
}

/// A group of events for one action. A mulligan has separate groups for the
/// return, shuffle, and draw. Undo visits groups in reverse order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AnimationMember {
    pub events: Vec<AnimationEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ShownCard {
    pub id: ObjectId,
    pub card: Card,
    pub face: Face,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnimationEvent {
    /// The card keeps the same id. Locations refer to the running zone order.
    Move {
        id: ObjectId,
        from: Location,
        to: Location,
    },
    /// The move changes the object's id. For the hand owner the card is
    /// identified only on the hand side. Other viewers animate a card back
    /// between the zones, without either id or a slot in the hand.
    HandMove {
        from: Zone,
        to: Zone,
        hand_card: Option<ShownCard>,
        /// The slot may follow the id change only for the hand's owner.
        #[serde(skip)]
        owner_transfer: Option<(ObjectId, ObjectId)>,
    },
    /// The library order to install after the shuffle. The shuffled cards
    /// have fresh ids and the event carries no mapping from the old order.
    Shuffle {
        library: Zone,
        order: Vec<ObjectId>,
    },
    /// The acting seat sees the selected cards. Other seats get the count.
    Look {
        library: Zone,
        count: usize,
        cards: Vec<ShownCard>,
    },
    /// Only the seats named as recipients get this event.
    Reveal {
        zone: Zone,
        cards: Vec<ShownCard>,
    },
    /// The viewer learned an identity. A later batch member can hide it again.
    Learn(ShownCard),
    /// The adapter takes a fresh view of the table.
    Reset,
    Phase(Phase),
    /// Redacted edits, creations, removals and public rolls.
    Change(Change),
}

/// Projects the entry using the host table immediately before it. Each member
/// resolves against the state made by the preceding members.
#[must_use]
pub fn animation_seen_by(entry: &Entry, table_before: &Table, viewer: Seat) -> AnimationProjection {
    let mut members = Vec::new();
    visit_members(
        entry,
        table_before,
        |presentation, changes, before, after| {
            members.push(AnimationMember {
                events: member_events(changes, presentation, entry.by, before, after, viewer),
            });
        },
    );
    AnimationProjection {
        n: entry.n,
        by: entry.by,
        members,
    }
}

/// Visits the animation events and the redacted result of each action member.
pub fn visit_animation(
    entry: &Entry,
    table_before: &Table,
    viewer: Seat,
    mut visit: impl FnMut(&[AnimationEvent], Table),
) {
    visit_members(
        entry,
        table_before,
        |presentation, changes, before, after| {
            let events = member_events(changes, presentation, entry.by, before, after, viewer);
            visit(&events, super::seen_by(after, viewer));
        },
    );
}

fn member_events(
    changes: &[Change],
    presentation: Option<&Presentation>,
    actor: Seat,
    before: &Table,
    after: &Table,
    viewer: Seat,
) -> Vec<AnimationEvent> {
    if matches!(presentation, Some(Presentation::Reset)) {
        return vec![AnimationEvent::Reset];
    }

    let mut names = HiddenNames::new(before, viewer);
    names.include_table(after, viewer);
    let private_ids = forgotten_ids(changes, before, after, viewer);
    let replacements = Replacements::new(changes);
    let mut shuffle = match presentation {
        Some(Presentation::Shuffle { library }) => Some(AnimationEvent::Shuffle {
            library: *library,
            order: after.zone(*library).expect("shuffled library").to_vec(),
        }),
        _ => None,
    };
    let mut events = Vec::new();
    for change in changes {
        match change {
            Change::Move { id, from, to }
                if !matches!(presentation, Some(Presentation::Shuffle { .. })) =>
            {
                events.push(replacements.movement(
                    *id,
                    *from,
                    *to,
                    (before, after),
                    viewer,
                    &names,
                ));
            }
            Change::Move { .. } | Change::ReplaceId { .. } => {}
            Change::Learn { id, seat } => {
                if *seat == viewer
                    && let Some(card) = shown_card(after, *id, viewer, &names)
                {
                    events.extend(shuffle.take());
                    events.push(AnimationEvent::Learn(card));
                }
            }
            Change::SetObject { id, .. } if replacements.contains(*id) => {}
            change => {
                let mut change = change.clone();
                if project_change(&mut change, before, after, viewer, &names, &private_ids) {
                    events.push(AnimationEvent::Change(change));
                }
            }
        }
    }

    events.extend(shuffle);
    let shown = |ids: &[ObjectId]| {
        ids.iter()
            .filter_map(|&id| shown_card(after, id, viewer, &names))
            .collect()
    };
    match presentation {
        Some(Presentation::Phase(phase)) => events.push(AnimationEvent::Phase(*phase)),
        Some(Presentation::Look { library, ids }) => events.push(AnimationEvent::Look {
            library: *library,
            count: ids.len(),
            cards: if viewer == actor {
                shown(ids)
            } else {
                Vec::new()
            },
        }),
        Some(Presentation::Reveal { zone, ids, to }) if to.contains(&viewer) => {
            events.push(AnimationEvent::Reveal {
                zone: *zone,
                cards: shown(ids),
            });
        }
        _ => {}
    }
    events
}

#[derive(Default)]
struct Replacements {
    by_old_id: BTreeMap<ObjectId, ObjectId>,
    by_new_id: BTreeMap<ObjectId, ObjectId>,
}

impl Replacements {
    fn new(changes: &[Change]) -> Self {
        let mut replacements = Self::default();
        for change in changes {
            if let Change::ReplaceId { from, to } = change {
                replacements.by_old_id.insert(*from, *to);
                replacements.by_new_id.insert(*to, *from);
            }
        }
        replacements
    }

    fn contains(&self, id: ObjectId) -> bool {
        self.by_old_id.contains_key(&id) || self.by_new_id.contains_key(&id)
    }

    fn movement(
        &self,
        id: ObjectId,
        from: Location,
        to: Location,
        (before, after): (&Table, &Table),
        viewer: Seat,
        names: &HiddenNames,
    ) -> AnimationEvent {
        let (owner, hand_id, state, transfer) = if let Zone::Hand(owner) = to.zone
            && let Some(&assigned) = self.by_old_id.get(&id)
        {
            (owner, assigned, after, (id, assigned))
        } else if let Zone::Hand(owner) = from.zone
            && let Some(&previous) = self.by_new_id.get(&id)
        {
            (owner, previous, before, (previous, id))
        } else {
            return AnimationEvent::Move { id, from, to };
        };
        AnimationEvent::HandMove {
            owner_transfer: (viewer == owner).then_some(transfer),
            from: from.zone,
            to: to.zone,
            hand_card: if viewer == owner {
                shown_card(state, hand_id, viewer, names)
            } else {
                None
            },
        }
    }
}

fn shown_card(table: &Table, id: ObjectId, viewer: Seat, names: &HiddenNames) -> Option<ShownCard> {
    let object = table.objects.get(&id)?;
    let mut card = if object.known_to.contains(&viewer) {
        object.card.clone()
    } else {
        Card::Hidden
    };
    redact_projected_card(&mut card, names);
    Some(ShownCard {
        id,
        card,
        face: object.face,
    })
}
