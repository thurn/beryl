use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::change::{Location, ObjectEdit, TableEdit};
use crate::state::{Arrow, ArrowTarget, ObjectId, Zone};
use crate::{Change, Seat, Table};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Field {
    Life(Seat),
    Counters(Seat),
    Active,
    Turn,
    Phase,
    ResetTurnPlays,
    TurnPlay,
    Assists(Seat),
    Deck(Seat),
    Sideboard(Seat),
    Ready(Seat),
    Wins(Seat),
    InGame,
    LastWinner,
    Arrows,
    Lends,
    RevealTop,
    LookAtTop,
}

impl From<&TableEdit> for Field {
    fn from(edit: &TableEdit) -> Self {
        match *edit {
            TableEdit::SetLife { seat, .. } => Self::Life(seat),
            TableEdit::SetPlayerCounters { seat, .. } => Self::Counters(seat),
            TableEdit::SetActive { .. } => Self::Active,
            TableEdit::SetTurn { .. } => Self::Turn,
            TableEdit::SetPhase { .. } => Self::Phase,
            TableEdit::ResetTurnPlays { .. } => Self::ResetTurnPlays,
            TableEdit::TurnPlay { .. } => Self::TurnPlay,
            TableEdit::SetAssists { seat, .. } => Self::Assists(seat),
            TableEdit::SetDeck { seat, .. } => Self::Deck(seat),
            TableEdit::SetSideboard { seat, .. } => Self::Sideboard(seat),
            TableEdit::SetReady { seat, .. } => Self::Ready(seat),
            TableEdit::SetWins { seat, .. } => Self::Wins(seat),
            TableEdit::SetInGame { .. } => Self::InGame,
            TableEdit::SetLastWinner { .. } => Self::LastWinner,
        }
    }
}

/// The positions whose indices or occupants an entry changed, including
/// positions shifted when a card enters or leaves a zone.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Touches {
    pub objects: BTreeSet<ObjectId>,
    pub positions: BTreeSet<Location>,
    fields: BTreeSet<Field>,
    whole_table: bool,
}

impl Touches {
    pub(crate) fn conflicts(&self, other: &Self) -> bool {
        self.whole_table
            || other.whole_table
            || !self.objects.is_disjoint(&other.objects)
            || !self.positions.is_disjoint(&other.positions)
            || self.fields.iter().any(|field| match field {
                // Independent plays can be undone in either order. A turn
                // boundary resets their count and conflicts with all plays.
                Field::TurnPlay => other.fields.contains(&Field::ResetTurnPlays),
                Field::ResetTurnPlays => {
                    other.fields.contains(&Field::TurnPlay) || other.fields.contains(field)
                }
                _ => other.fields.contains(field),
            })
    }

    fn positions(&mut self, zone: Zone, start: usize, end: usize) {
        self.positions
            .extend((start..end).map(|index| Location { zone, index }));
    }

    fn remove_at(&mut self, table: &Table, location: Location) {
        let len = table.zone(location.zone).expect("changed zone").len();
        // An inverse inserts at the former index. A later removal can
        // invalidate that index even by shrinking an already shorter zone.
        self.positions(location.zone, location.index, len + 1);
    }

    fn insert_at(&mut self, table: &Table, location: Location) {
        let len = table.zone(location.zone).expect("changed zone").len();
        self.positions(location.zone, location.index, len + 1);
    }

    fn arrow(&mut self, arrow: &Arrow) {
        self.objects.insert(arrow.from);
        if let ArrowTarget::Object(id) = arrow.to {
            self.objects.insert(id);
        }
        self.fields.insert(Field::Arrows);
    }

    pub(crate) fn record(&mut self, table: &Table, change: &Change) {
        match change {
            Change::Move { id, from, to } => {
                self.objects.insert(*id);
                if from.zone == to.zone {
                    self.positions(
                        from.zone,
                        from.index.min(to.index),
                        from.index.max(to.index) + 1,
                    );
                } else {
                    self.remove_at(table, *from);
                    self.insert_at(table, *to);
                }
            }
            Change::SetObject { id, edit } => {
                self.objects.insert(*id);
                if let ObjectEdit::AttachedTo(edit) = edit {
                    self.objects.extend(edit.before);
                    self.objects.extend(edit.after);
                }
            }
            Change::Learn { id, .. } | Change::ResetKnowledge { id, .. } => {
                self.objects.insert(*id);
            }
            Change::ReplaceId { from, to } => {
                self.objects.extend([*from, *to]);
                let (zone, index) = table.position(*from).expect("replacement source");
                self.positions.insert(Location { zone, index });
            }
            Change::Create { object, index } | Change::Remove { object, index } => {
                self.objects.insert(object.id);
                self.objects.extend(object.attached_to);
                let location = Location {
                    zone: object.zone,
                    index: *index,
                };
                if matches!(change, Change::Create { .. }) {
                    self.insert_at(table, location);
                } else {
                    self.remove_at(table, location);
                }
            }
            Change::CreateMarker { id, marker, index }
            | Change::RemoveMarker { id, marker, index } => {
                self.objects.extend([*id, marker.source]);
                let location = Location {
                    zone: Zone::Stack,
                    index: *index,
                };
                if matches!(change, Change::CreateMarker { .. }) {
                    self.insert_at(table, location);
                } else {
                    self.remove_at(table, location);
                }
            }
            Change::InsertArrow { arrow, .. } | Change::RemoveArrow { arrow, .. } => {
                self.arrow(arrow);
            }
            Change::SetTable(edit) => {
                self.fields.insert(edit.into());
            }
            Change::Lend { .. } => {
                self.fields.insert(Field::Lends);
            }
            Change::AlwaysRevealTop { .. } => {
                self.fields.insert(Field::RevealTop);
            }
            Change::AlwaysLookAtTop { .. } => {
                self.fields.insert(Field::LookAtTop);
            }
            Change::RestoreTable(_) => self.whole_table = true,
            Change::Random { .. } | Change::Roll { .. } | Change::Nothing => {}
        }
    }

    pub(crate) fn project(&mut self, private_ids: &BTreeSet<ObjectId>, table: &Table, seat: Seat) {
        // Private position metadata can reveal which slots a shuffle or draw
        // rearranged even after its moves are removed.
        self.positions.retain(|location| location.zone.is_public());
        self.fields.retain(|field| match *field {
            Field::Deck(owner) | Field::Sideboard(owner) => owner == seat,
            _ => true,
        });
        self.objects.retain(|id| {
            !private_ids.contains(id)
                && (table.objects.contains_key(id) || table.markers.contains_key(id))
        });
    }
}
