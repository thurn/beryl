//! Placement: for a view and a window, the pose every object rests in, by one function per zone
//! (docs/beryl-3d.md, "Objects and placement"). Each is pure: it reads the view, the layout, the
//! camera fitted to the window and the rows' scroll offsets, and nothing else.

use beryl_protocol::protocol::{
    CardType, ObjectId, ObjectView, PlayerState, PlayerView, RowName, Side, StackItem, ZoneName,
};
use beryl_protocol::script::{Piles, Table as Played};
use glam::{Quat, Vec2, Vec3};

use super::rows::{Laid, RowRules, by_name, face_up, lay_row};
use super::{CARD_HEIGHT, CARD_WIDTH, Camera, HandData, LAYER, Layout};
use crate::frame::Place;
use crate::pose::Pose;

/// Where one object rests.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rest {
    pub pose: Pose,
    pub place: Place,
    /// False for a card scrolled out of its row's view, which is not drawn.
    pub visible: bool,
}

/// The rows of one side, in the order of `Placement::scroll`'s entries.
pub const ROWS: [RowName; 3] = [RowName::Creatures, RowName::Lands, RowName::Others];

/// Every object's resting place, by slot.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Placement {
    /// Indexed by `ObjectId`; none for an object the table does not show (in a sideboard, or
    /// nowhere).
    pub rests: Vec<Option<Rest>>,
}

impl Placement {
    #[must_use]
    pub fn get(&self, slot: ObjectId) -> Option<&Rest> {
        self.rests.get(usize::from(slot.0))?.as_ref()
    }

    /// Forgets where the object in `slot` rests.
    pub(crate) fn unset(&mut self, slot: ObjectId) {
        if let Some(rest) = self.rests.get_mut(usize::from(slot.0)) {
            *rest = None;
        }
    }

    pub(crate) fn set(&mut self, slot: ObjectId, rest: Rest) {
        let index = usize::from(slot.0);
        if self.rests.len() <= index {
            self.rests.resize(index + 1, None);
        }
        self.rests[index] = Some(rest);
    }
}

/// The scroll offset of each row, in groups from its left end: for the viewer and then the
/// opponent, each in the order of `ROWS`.
pub type Scroll = [[usize; 3]; 2];

/// Every object's resting place in `view`, seen through `camera`.
#[must_use]
pub fn place(view: &PlayerView, layout: &Layout, camera: &Camera, scroll: &Scroll) -> Placement {
    place_zones(&Zones::of_view(view), layout, camera, scroll)
}

/// Every object's resting place in `zones`, seen through `camera`.
pub(crate) fn place_zones(
    zones: &Zones,
    layout: &Layout,
    camera: &Camera,
    scroll: &Scroll,
) -> Placement {
    let mut placement = Placement::default();
    for group in Group::ALL {
        lay(
            group,
            &members(zones, group),
            layout,
            camera,
            scroll,
            &mut placement,
        );
    }
    placement
}

/// Places the objects of `group` in `zones` into `placement`, and leaves the slots it placed in
/// `placed`. Other groups' objects keep their places: no group's resting places depend on
/// another's objects.
pub(crate) fn place_group(
    zones: &Zones,
    group: Group,
    rules: (&Layout, &Camera, &Scroll),
    placement: &mut Placement,
    placed: &mut Vec<ObjectId>,
) {
    let (layout, camera, scroll) = rules;
    let objects = members(zones, group);
    placed.clear();
    placed.extend(objects.iter().map(|object| object.slot));
    lay(group, &objects, layout, camera, scroll, placement);
}

/// A part of the table whose objects are placed together and apart from every other part: a
/// pile, a hand, a battlefield row or the stack. Where an object rests depends only on the
/// objects of its own group, so an act changes the resting places of the groups that held or
/// now hold the objects it names, and no others.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Group {
    /// A library, graveyard or exile pile.
    Pile {
        side: Side,
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
            side,
            zone: ZoneName::Library,
        },
        Group::Pile {
            side,
            zone: ZoneName::Graveyard,
        },
        Group::Pile {
            side,
            zone: ZoneName::Exile,
        },
        Group::Hand(side),
        Group::Row { side, row: ROWS[0] },
        Group::Row { side, row: ROWS[1] },
        Group::Row { side, row: ROWS[2] },
    ]
}

impl Group {
    /// Every group, in the order `place` lays them out: the viewer's, the opponent's and then
    /// the stack.
    pub(crate) const ALL: [Group; 15] = {
        let [a0, a1, a2, a3, a4, a5, a6] = side_groups(Side::You);
        let [b0, b1, b2, b3, b4, b5, b6] = side_groups(Side::Opponent);
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
        ]
    };

    /// Its place in `ALL`.
    pub(crate) fn index(self) -> usize {
        let first = |side| match side {
            Side::You => 0,
            Side::Opponent => 7,
        };
        match self {
            Group::Pile { side, zone } => {
                first(side)
                    + match zone {
                        ZoneName::Library => 0,
                        ZoneName::Graveyard => 1,
                        _ => 2,
                    }
            }
            Group::Hand(side) => first(side) + 3,
            Group::Row { side, row } => first(side) + 4 + row_index(row),
            Group::Stack => 14,
        }
    }

    /// The group `object` belongs in, by its zone, if the table shows it.
    pub(crate) fn of(object: &ObjectView) -> Option<Group> {
        let side = side_of(object);
        Some(match object.zone {
            ZoneName::Library | ZoneName::Graveyard | ZoneName::Exile => Group::Pile {
                side,
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

/// `row`'s place in `ROWS`.
fn row_index(row: RowName) -> usize {
    ROWS.iter()
        .position(|other| *other == row)
        .expect("every row is in `ROWS`")
}

/// `side`'s place in `Scroll` and `Zones::piles`: the viewer first.
fn side_index(side: Side) -> usize {
    match side {
        Side::You => 0,
        Side::Opponent => 1,
    }
}

/// What placement reads of a table, borrowed from where it is kept: a view, or the table a
/// script is played over.
pub(crate) struct Zones<'a> {
    /// Every object, in the order hands and exile piles list them.
    objects: Vec<&'a ObjectView>,
    /// The same objects, by slot.
    slots: Vec<Option<&'a ObjectView>>,
    /// Permanents, in the order they entered.
    battlefield: &'a [ObjectId],
    /// The spells on the stack, bottom first.
    spells: Vec<ObjectId>,
    /// Each side's library and graveyard, bottom first: the viewer's and then the opponent's.
    piles: [[&'a [ObjectId]; 2]; 2],
}

impl<'a> Zones<'a> {
    pub(crate) fn of_view(view: &'a PlayerView) -> Self {
        let player = |player: &'a PlayerState| [&player.library[..], &player.graveyard[..]];
        Self::new(
            view.objects.iter().collect(),
            &view.battlefield,
            view.stack.iter(),
            [player(&view.players.you), player(&view.players.opponent)],
        )
    }

    /// The zones of the table a script plays over. It keeps its objects by slot, which is the
    /// order its hands and exile piles list them in.
    pub(crate) fn of_table(table: &'a Played) -> Self {
        let piles = |piles: &'a Piles| [&piles.library[..], &piles.graveyard[..]];
        Self::new(
            table.objects.values().collect(),
            &table.battlefield,
            table.stack.iter().map(|entry| &entry.item),
            [piles(&table.you), piles(&table.opponent)],
        )
    }

    fn new(
        objects: Vec<&'a ObjectView>,
        battlefield: &'a [ObjectId],
        stack: impl Iterator<Item = &'a StackItem>,
        piles: [[&'a [ObjectId]; 2]; 2],
    ) -> Self {
        let mut slots = Vec::new();
        for &object in &objects {
            let index = usize::from(object.slot.0);
            if slots.len() <= index {
                slots.resize(index + 1, None);
            }
            slots[index] = Some(object);
        }
        let spells = stack
            .filter_map(|item| match item {
                StackItem::Spell { slot, .. } => Some(*slot),
                StackItem::Ability { .. } => None,
            })
            .collect();
        Self {
            objects,
            slots,
            battlefield,
            spells,
            piles,
        }
    }

    /// The object in `slot`, if the table holds one there.
    pub(crate) fn get(&self, slot: ObjectId) -> Option<&'a ObjectView> {
        self.slots.get(usize::from(slot.0)).copied().flatten()
    }

    /// The objects in one side's `zone`, in the order the table lists them.
    fn in_zone(&self, side: Side, zone: ZoneName) -> Vec<&'a ObjectView> {
        self.objects
            .iter()
            .copied()
            .filter(|object| object.zone == zone && side_of(object) == side)
            .collect()
    }

    /// The objects in `slots`, in that order, leaving out any slot the table holds nothing in.
    fn listed(&self, slots: &[ObjectId]) -> Vec<&'a ObjectView> {
        slots.iter().filter_map(|&slot| self.get(slot)).collect()
    }
}

/// The objects of `group`, in the order it lays them out from.
fn members<'a>(zones: &Zones<'a>, group: Group) -> Vec<&'a ObjectView> {
    match group {
        Group::Pile {
            side,
            zone: ZoneName::Library,
        } => zones.listed(zones.piles[side_index(side)][0]),
        Group::Pile {
            side,
            zone: ZoneName::Graveyard,
        } => zones.listed(zones.piles[side_index(side)][1]),
        Group::Pile { side, zone } => zones.in_zone(side, zone),
        Group::Hand(side) => zones.in_zone(side, ZoneName::Hand),
        Group::Row { side, row } => zones
            .battlefield
            .iter()
            .filter_map(|&slot| zones.get(slot))
            .filter(|object| side_of(object) == side && row_of(object) == row)
            .collect(),
        Group::Stack => zones.listed(&zones.spells),
    }
}

/// Places `objects`, the members of `group`, into `placement`.
fn lay(
    group: Group,
    objects: &[&ObjectView],
    layout: &Layout,
    camera: &Camera,
    scroll: &Scroll,
    placement: &mut Placement,
) {
    match group {
        Group::Pile { side, zone } => {
            let at = match zone {
                ZoneName::Library => layout.library,
                ZoneName::Graveyard => layout.graveyard,
                _ => layout.exile,
            };
            pile(placement, objects, zone, side, at);
        }
        Group::Hand(side) => {
            let data = match side {
                Side::You => layout.hand,
                Side::Opponent => layout.opponent_hand,
            };
            hand(placement, objects, side, data, camera);
        }
        Group::Row { side, row } => {
            let offset = scroll[side_index(side)][row_index(row)];
            battlefield_row(placement, objects, side, row, layout, camera, offset);
        }
        Group::Stack => stack(placement, objects, layout),
    }
}

/// Whose side of the table an object is on: a permanent's controller's, and otherwise the
/// player whose zone holds it.
fn side_of(object: &ObjectView) -> Side {
    if object.zone == ZoneName::Battlefield {
        object.controller
    } else {
        object.zone_side.unwrap_or(object.owner)
    }
}

/// A point of the viewer's side on the opponent's: turned half a turn about the table's centre.
fn turned(at: Vec2, side: Side) -> Vec2 {
    match side {
        Side::You => at,
        Side::Opponent => -at,
    }
}

/// The turn of a card that shows its face (0) or its back (1).
fn turn(object: &ObjectView) -> f32 {
    if face_up(object) { 0.0 } else { 1.0 }
}

/// A library, graveyard or exile pile: bottom first, each card one `LAYER` above the last. A
/// library is always face down.
fn pile(placement: &mut Placement, objects: &[&ObjectView], zone: ZoneName, side: Side, at: Vec2) {
    let at = turned(at, side);
    for (height, object) in objects.iter().enumerate() {
        let pose = Pose {
            position: at.extend(LAYER * (height as f32 + 0.5)),
            turn: if zone == ZoneName::Library {
                1.0
            } else {
                turn(object)
            },
            ..Pose::default()
        };
        let place = Place::Card {
            zone,
            side,
            row: None,
        };
        placement.set(
            object.slot,
            Rest {
                pose,
                place,
                visible: true,
            },
        );
    }
}

/// A hand, fanned along an edge of the screen and placed relative to the camera so that it
/// keeps its size and place at any window shape. Each card lies in a plane parallel to the
/// screen, a little nearer the eye than the card before it, so neighbours cover each other
/// cleanly. The viewer's runs along the bottom edge; the opponent's along the top, mostly out of
/// view.
fn hand(
    placement: &mut Placement,
    objects: &[&ObjectView],
    side: Side,
    data: HandData,
    camera: &Camera,
) {
    let count = objects.len();
    let aspect = camera.window.size.width / camera.window.size.height;
    let card = data.height * 2.0 * (CARD_WIDTH / CARD_HEIGHT) / aspect;
    let spacing = (card * 0.8).min(data.width / count.max(1) as f32);
    // The opponent's hand faces them: its fan turns the other way, and it recedes as it goes.
    let (lean, nearer) = match side {
        Side::You => (-1.0, 1.0),
        Side::Opponent => (1.0, -1.0),
    };
    for (index, object) in objects.iter().enumerate() {
        let offset = index as f32 - (count as f32 - 1.0) / 2.0;
        let ndc = Vec2::new(offset * spacing, data.y + lean * offset * offset * 0.003);
        let rotation = Quat::from_rotation_z(lean * offset * data.fan);
        let depth = data.depth - nearer * index as f32 * 1.5;
        let mut pose = camera.screen_pose(ndc, depth, rotation, data.height);
        pose.turn = turn(object);
        let place = Place::Card {
            zone: ZoneName::Hand,
            side,
            row: None,
        };
        placement.set(
            object.slot,
            Rest {
                pose,
                place,
                visible: true,
            },
        );
    }
}

/// Which row a permanent stands in: where a player pinned it, else the creatures' row for a
/// creature, the lands' part of the back row for a land, and the others' part for the rest.
fn row_of(object: &ObjectView) -> RowName {
    let creature = object.types.contains(&CardType::Creature);
    object.pinned.unwrap_or(if creature {
        RowName::Creatures
    } else if object.types.contains(&CardType::Land) {
        RowName::Lands
    } else {
        RowName::Others
    })
}

/// One of one side's three battlefield rows, `cards` in the order they entered. They keep that
/// order, except that lands stand untapped first, so how much mana is open reads at a glance,
/// and each name together, in the order the names first appear.
fn battlefield_row(
    placement: &mut Placement,
    cards: &[&ObjectView],
    side: Side,
    row: RowName,
    layout: &Layout,
    camera: &Camera,
    offset: usize,
) {
    let area = match row {
        RowName::Creatures => layout.creatures,
        RowName::Lands => layout.lands,
        RowName::Others => layout.others,
    };
    let sorted;
    let cards = if row == RowName::Lands {
        sorted = lands_in_order(cards);
        &sorted[..]
    } else {
        cards
    };
    let y = match side {
        Side::You => area.y,
        Side::Opponent => -area.y,
    };
    let strip = layout.strip / camera.points_per_millimetre(Vec3::new(0.0, y, 0.0));
    let rules = RowRules {
        layout,
        strip,
        scroll: offset,
    };
    for Laid {
        object,
        pose,
        visible,
    } in lay_row(cards, area, y, &rules)
    {
        let place = Place::Card {
            zone: ZoneName::Battlefield,
            side,
            row: Some(row),
        };
        placement.set(
            object.slot,
            Rest {
                pose,
                place,
                visible,
            },
        );
    }
}

/// A lands row's cards in the order they stand: untapped first, and each name together, in the
/// order the names first appear in `cards`. Otherwise the order of `cards` is kept.
fn lands_in_order<'a>(cards: &[&'a ObjectView]) -> Vec<&'a ObjectView> {
    // The index of the first card with each card's name.
    let mut first = vec![0; cards.len()];
    for run in by_name(cards).chunk_by(|&a, &b| cards[a].name == cards[b].name) {
        for &index in run {
            first[index] = run[0];
        }
    }
    let mut order: Vec<usize> = (0..cards.len()).collect();
    order.sort_by_key(|&index| (cards[index].tapped, first[index]));
    order.into_iter().map(|index| cards[index]).collect()
}

/// The stack's spells, `spells` bottom first, raised above the table at its right: each one
/// nearer the viewer and higher than the one beneath. Abilities on the stack are drawn by the
/// flat layer.
fn stack(placement: &mut Placement, spells: &[&ObjectView], layout: &Layout) {
    for (index, object) in spells.iter().enumerate() {
        let pose = Pose {
            position: layout.stack + layout.stack_step * index as f32,
            turn: turn(object),
            ..Pose::default()
        };
        let place = Place::Card {
            zone: ZoneName::Stack,
            side: object.controller,
            row: None,
        };
        placement.set(
            object.slot,
            Rest {
                pose,
                place,
                visible: true,
            },
        );
    }
}
