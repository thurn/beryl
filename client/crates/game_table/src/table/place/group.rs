//! Placement groups: each pile, hand, battlefield row and the stack.

use beryl_protocol::protocol::{ObjectView, RowName, Side, ZoneName};

use super::{Place, ROWS, in_hand_fan, row_index, row_of, side_of};
use crate::table::TableShape;

/// What `Zones::of_table_grouped` holds for a slot with no object, or one the table does not
/// show: no `Group::number`.
pub const NO_GROUP: u8 = u8::MAX;

/// A part of the table whose objects are placed together and apart from every other part: a
/// pile, a hand, a battlefield row or the stack. Rows also read the pile rules, since the
/// stack names some permanents and lands in other rows count toward the land threshold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// A library, graveyard or exile pile (`None` for the shared piles of a shared-deck table).
    Pile {
        side: Option<Side>,
        zone: ZoneName,
    },
    Hand(Side),
    Row {
        side: Side,
        row: RowName,
    },
    Stack,
}

/// The groups of one side, in the order `place` lays them out.
const fn side_groups(side: Side) -> [Group; 7] {
    [
        Group::Pile {
            side: Some(side),
            zone: ZoneName::Library,
        },
        Group::Pile {
            side: Some(side),
            zone: ZoneName::Graveyard,
        },
        Group::Pile {
            side: Some(side),
            zone: ZoneName::Exile,
        },
        Group::Hand(side),
        Group::Row { side, row: ROWS[0] },
        Group::Row { side, row: ROWS[1] },
        Group::Row { side, row: ROWS[2] },
    ]
}

/// The three shared piles of a shared-deck table.
const SHARED_PILES: [Group; 3] = [
    Group::Pile {
        side: None,
        zone: ZoneName::Library,
    },
    Group::Pile {
        side: None,
        zone: ZoneName::Graveyard,
    },
    Group::Pile {
        side: None,
        zone: ZoneName::Exile,
    },
];

impl Group {
    /// Every group, in the order `place` lays them out: the viewer's, the opponent's, the stack,
    /// and the three shared piles.
    pub const ALL: [Group; 18] = {
        let [a0, a1, a2, a3, a4, a5, a6] = side_groups(Side::You);
        let [b0, b1, b2, b3, b4, b5, b6] = side_groups(Side::Opponent);
        let [s0, s1, s2] = SHARED_PILES;
        [
            a0,
            a1,
            a2,
            a3,
            a4,
            a5,
            a6,
            b0,
            b1,
            b2,
            b3,
            b4,
            b5,
            b6,
            Group::Stack,
            s0,
            s1,
            s2,
        ]
    };

    /// Whether `self` belongs to `shape`.
    #[must_use]
    pub const fn in_shape(self, shape: TableShape) -> bool {
        !matches!(
            (self, shape),
            (Group::Pile { side: None, .. }, TableShape::PerPlayerPiles)
                | (Group::Pile { side: Some(_), .. }, TableShape::SharedPiles)
        )
    }

    /// `index`, as one byte.
    pub fn number(self) -> u8 {
        u8::try_from(self.index()).expect("fewer than 256 groups")
    }

    /// Its place in `ALL`.
    pub fn index(self) -> usize {
        let first = |side| match side {
            Side::You => 0,
            Side::Opponent => 7,
        };
        let zone_slot = |zone| match zone {
            ZoneName::Library => 0,
            ZoneName::Graveyard => 1,
            _ => 2,
        };
        match self {
            Group::Pile {
                side: Some(side),
                zone,
            } => first(side) + zone_slot(zone),
            Group::Pile { side: None, zone } => 15 + zone_slot(zone),
            Group::Hand(side) => first(side) + 3,
            Group::Row { side, row } => first(side) + 4 + row_index(row),
            Group::Stack => 14,
        }
    }

    /// Whether it lays its objects out in the order the table lists every object, which a view
    /// and the table a script plays over list differently: by the view's order, and by slot.
    /// The other groups follow lists the two keep alike (the battlefield's, a library's or
    /// graveyard's) or, the stack, are placed again after every step.
    pub fn in_object_order(self) -> bool {
        matches!(
            self,
            Group::Hand(_)
                | Group::Pile {
                    zone: ZoneName::Exile,
                    ..
                }
        )
    }

    /// The group of an object resting at `place`, which only that group's placement gives.
    pub fn at(place: Place) -> Option<Group> {
        match place {
            Place::SharedPile(zone) => Some(Group::Pile { side: None, zone }),
            Place::Card { zone, side, row } => Some(match (zone, row) {
                // A card in exile that lies under a permanent rests in that permanent's row.
                (ZoneName::Battlefield | ZoneName::Merged | ZoneName::Exile, Some(row)) => {
                    Group::Row { side, row }
                }
                (ZoneName::Library | ZoneName::Graveyard | ZoneName::Exile, _) => Group::Pile {
                    side: Some(side),
                    zone,
                },
                (ZoneName::Hand, _) => Group::Hand(side),
                (ZoneName::Stack, _) => Group::Stack,
                _ => return None,
            }),
            Place::Layer | Place::Revealed => None,
        }
    }

    /// The group `object` belongs in on a per-player table, by its zone, if the table shows it.
    pub fn of(object: &ObjectView) -> Option<Group> {
        Self::of_in(object, false)
    }

    /// The group `object` belongs in, by its zone and whether the table shares its piles.
    /// A card that lies under a permanent (a merged permanent's, or one it holds in exile)
    /// belongs in that permanent's row, which only `Zones::group_of` can find.
    pub fn of_in(object: &ObjectView, shared: bool) -> Option<Group> {
        let side = side_of(object);
        if in_hand_fan(object) {
            return Some(Group::Hand(Side::You));
        }
        Some(match object.zone {
            ZoneName::Library | ZoneName::Graveyard | ZoneName::Exile => Group::Pile {
                side: (!shared).then_some(side),
                zone: object.zone,
            },
            ZoneName::Hand => Group::Hand(side),
            ZoneName::Battlefield => Group::Row {
                side,
                row: row_of(object),
            },
            ZoneName::Stack => Group::Stack,
            _ => return None,
        })
    }
}
