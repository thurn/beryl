use serde::{Deserialize, Serialize};

use crate::state::{AbilityMarker, Arrow, Face, Object, ObjectId, Row, Seat, Table, Zone};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub zone: Zone,
    pub index: usize,
}

/// A field edit stores both values so the same primitive can apply or undo it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edit<T> {
    pub before: T,
    pub after: T,
}

impl<T: Clone> Edit<T> {
    #[must_use]
    pub fn inverse(&self) -> Self {
        Self {
            before: self.after.clone(),
            after: self.before.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectEdit {
    Controller(Edit<Seat>),
    Face(Edit<Face>),
    Tapped(Edit<bool>),
    Counters(Edit<Vec<(String, i32)>>),
    Stats(Edit<Option<(i32, i32)>>),
    Damage(Edit<i32>),
    Note(Edit<Option<String>>),
    AttachedTo(Edit<Option<ObjectId>>),
    DoesntUntap(Edit<bool>),
    Row(Edit<Row>),
}

impl ObjectEdit {
    fn perform(&self, object: &mut Object) {
        match self {
            Self::Controller(edit) => object.controller = edit.after,
            Self::Face(edit) => object.face = edit.after,
            Self::Tapped(edit) => object.tapped = edit.after,
            Self::Counters(edit) => object.counters.clone_from(&edit.after),
            Self::Stats(edit) => object.stats = edit.after,
            Self::Damage(edit) => object.damage = edit.after,
            Self::Note(edit) => object.note.clone_from(&edit.after),
            Self::AttachedTo(edit) => object.attached_to = edit.after,
            Self::DoesntUntap(edit) => object.doesnt_untap = edit.after,
            Self::Row(edit) => object.row = edit.after,
        }
    }

    fn inverse(&self) -> Self {
        match self {
            Self::Controller(edit) => Self::Controller(edit.inverse()),
            Self::Face(edit) => Self::Face(edit.inverse()),
            Self::Tapped(edit) => Self::Tapped(edit.inverse()),
            Self::Counters(edit) => Self::Counters(edit.inverse()),
            Self::Stats(edit) => Self::Stats(edit.inverse()),
            Self::Damage(edit) => Self::Damage(edit.inverse()),
            Self::Note(edit) => Self::Note(edit.inverse()),
            Self::AttachedTo(edit) => Self::AttachedTo(edit.inverse()),
            Self::DoesntUntap(edit) => Self::DoesntUntap(edit.inverse()),
            Self::Row(edit) => Self::Row(edit.inverse()),
        }
    }
}

/// A primitive at the point when it is applied. Position indices refer to the
/// state after all earlier changes in the same entry have been performed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Move {
        id: ObjectId,
        from: Location,
        to: Location,
    },
    SetObject {
        id: ObjectId,
        edit: ObjectEdit,
    },
    Learn {
        id: ObjectId,
        seat: Seat,
    },
    /// Hand entries and shuffles reset visibility. The inverse restores the prior
    /// visibility while retaining what was learned after the reset.
    ResetKnowledge {
        id: ObjectId,
        edit: Edit<Vec<Seat>>,
        forget: bool,
    },
    ReplaceId {
        from: ObjectId,
        to: ObjectId,
    },
    Create {
        object: Object,
        index: usize,
    },
    Remove {
        object: Object,
        index: usize,
    },
    CreateMarker {
        id: ObjectId,
        marker: AbilityMarker,
        index: usize,
    },
    RemoveMarker {
        id: ObjectId,
        marker: AbilityMarker,
        index: usize,
    },
    InsertArrow {
        arrow: Arrow,
        index: usize,
    },
    RemoveArrow {
        arrow: Arrow,
        index: usize,
    },
    Random {
        outcomes: Vec<u32>,
    },
    Nothing,
}

fn learn(known_to: &mut Vec<Seat>, seat: Seat) {
    if !known_to.contains(&seat) {
        known_to.push(seat);
    }
}

impl Change {
    /// Performs a change produced for this state. Callers performing an inverse
    /// must first check that intervening entries have left its inputs intact.
    pub fn perform(&self, table: &mut Table) {
        match self {
            Self::Move { id, from, to } => {
                let found = table
                    .zone_mut(from.zone)
                    .expect("source zone")
                    .remove(from.index);
                assert_eq!(found, *id);
                table
                    .zone_mut(to.zone)
                    .expect("destination zone")
                    .insert(to.index, *id);
                if let Some(object) = table.objects.get_mut(id) {
                    object.zone = to.zone;
                }
            }
            Self::SetObject { id, edit } => {
                edit.perform(table.objects.get_mut(id).expect("edited object"));
            }
            Self::Learn { id, seat } => {
                learn(
                    &mut table.objects.get_mut(id).expect("known object").known_to,
                    *seat,
                );
            }
            Self::ResetKnowledge { id, edit, forget } => {
                let known_to = &mut table.objects.get_mut(id).expect("known object").known_to;
                if *forget {
                    known_to.clone_from(&edit.after);
                } else {
                    let mut restored = edit.after.clone();
                    for &seat in known_to.iter() {
                        learn(&mut restored, seat);
                    }
                    *known_to = restored;
                }
            }
            Self::ReplaceId { from, to } => {
                let (zone, index) = table.position(*from).expect("replacement source");
                let mut object = table.objects.remove(from).expect("object whose id changed");
                object.id = *to;
                assert!(table.objects.insert(*to, object).is_none());
                table.zone_mut(zone).expect("replacement zone")[index] = *to;
            }
            Self::Create { object, index } => {
                table
                    .zone_mut(object.zone)
                    .expect("created object's zone")
                    .insert(*index, object.id);
                assert!(table.objects.insert(object.id, object.clone()).is_none());
            }
            Self::Remove { object, index } => {
                let found = table
                    .zone_mut(object.zone)
                    .expect("removed object's zone")
                    .remove(*index);
                assert_eq!(found, object.id);
                assert!(table.objects.remove(&object.id).is_some());
            }
            Self::CreateMarker { id, marker, index } => {
                table.stack.insert(*index, *id);
                assert!(table.markers.insert(*id, marker.clone()).is_none());
            }
            Self::RemoveMarker { id, index, .. } => {
                assert_eq!(table.stack.remove(*index), *id);
                assert!(table.markers.remove(id).is_some());
            }
            Self::InsertArrow { arrow, index } => table.arrows.insert(*index, arrow.clone()),
            Self::RemoveArrow { arrow, index } => assert_eq!(table.arrows.remove(*index), *arrow),
            Self::Random { .. } | Self::Nothing => {}
        }
    }

    #[must_use]
    pub fn inverse(&self) -> Self {
        match self {
            Self::Move { id, from, to } => Self::Move {
                id: *id,
                from: *to,
                to: *from,
            },
            Self::SetObject { id, edit } => Self::SetObject {
                id: *id,
                edit: edit.inverse(),
            },
            Self::ResetKnowledge { id, edit, .. } => Self::ResetKnowledge {
                id: *id,
                edit: edit.inverse(),
                forget: false,
            },
            Self::ReplaceId { from, to } => Self::ReplaceId {
                from: *to,
                to: *from,
            },
            Self::Create { object, index } => Self::Remove {
                object: object.clone(),
                index: *index,
            },
            Self::Remove { object, index } => Self::Create {
                object: object.clone(),
                index: *index,
            },
            Self::CreateMarker { id, marker, index } => Self::RemoveMarker {
                id: *id,
                marker: marker.clone(),
                index: *index,
            },
            Self::RemoveMarker { id, marker, index } => Self::CreateMarker {
                id: *id,
                marker: marker.clone(),
                index: *index,
            },
            Self::InsertArrow { arrow, index } => Self::RemoveArrow {
                arrow: arrow.clone(),
                index: *index,
            },
            Self::RemoveArrow { arrow, index } => Self::InsertArrow {
                arrow: arrow.clone(),
                index: *index,
            },
            Self::Learn { .. } | Self::Random { .. } | Self::Nothing => Self::Nothing,
        }
    }

    pub(crate) fn introduced_id(&self) -> Option<ObjectId> {
        match self {
            Self::ReplaceId { to, .. } => Some(*to),
            Self::Create { object, .. } => Some(object.id),
            Self::CreateMarker { id, .. } => Some(*id),
            _ => None,
        }
    }
}
