//! Placement: for a view and a window, the pose every object rests in, by one function per zone
//! (docs/beryl-3d.md, "Objects and placement"). Each is pure: it reads the view, the layout, the
//! camera fitted to the window and the rows' scroll offsets, and nothing else.

use beryl_session::protocol::{
    CardType, ObjectId, ObjectView, PlayerView, RowName, Side, StackItem, ZoneName,
};
use glam::{Quat, Vec2, Vec3};

use super::rows::{Laid, RowRules, face_up, lay_row};
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

    fn set(&mut self, slot: ObjectId, rest: Rest) {
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
    let mut placement = Placement::default();
    let slots = Slots::new(view);
    for (index, side) in [Side::You, Side::Opponent].into_iter().enumerate() {
        let player = match side {
            Side::You => &view.players.you,
            Side::Opponent => &view.players.opponent,
        };
        let piles = [
            (ZoneName::Library, &player.library, layout.library),
            (ZoneName::Graveyard, &player.graveyard, layout.graveyard),
        ];
        for (zone, ids, at) in piles {
            let objects: Vec<&ObjectView> =
                ids.iter().filter_map(|&slot| slots.get(slot)).collect();
            pile(&mut placement, &objects, zone, side, at);
        }
        let exile = in_zone(view, side, ZoneName::Exile);
        pile(&mut placement, &exile, ZoneName::Exile, side, layout.exile);
        let hand_cards = in_zone(view, side, ZoneName::Hand);
        let data = match side {
            Side::You => layout.hand,
            Side::Opponent => layout.opponent_hand,
        };
        hand(&mut placement, &hand_cards, side, data, camera);
        battlefield(
            &mut placement,
            view,
            &slots,
            side,
            layout,
            camera,
            &scroll[index],
        );
    }
    stack(&mut placement, view, &slots, layout);
    placement
}

/// The view's objects indexed by slot.
struct Slots<'a>(Vec<Option<&'a ObjectView>>);

impl<'a> Slots<'a> {
    fn new(view: &'a PlayerView) -> Self {
        let mut slots = Vec::new();
        for object in &view.objects {
            let index = usize::from(object.slot.0);
            if slots.len() <= index {
                slots.resize(index + 1, None);
            }
            slots[index] = Some(object);
        }
        Self(slots)
    }

    fn get(&self, slot: ObjectId) -> Option<&'a ObjectView> {
        self.0.get(usize::from(slot.0)).copied().flatten()
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

/// The objects in one side's `zone`, in the view's order.
fn in_zone(view: &PlayerView, side: Side, zone: ZoneName) -> Vec<&ObjectView> {
    view.objects
        .iter()
        .filter(|object| object.zone == zone && side_of(object) == side)
        .collect()
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

/// One side's three battlefield rows. Permanents keep the order they entered in, except that
/// lands stand untapped first, so how much mana is open reads at a glance, and each name
/// together, in the order the names first appear.
fn battlefield(
    placement: &mut Placement,
    view: &PlayerView,
    slots: &Slots,
    side: Side,
    layout: &Layout,
    camera: &Camera,
    scroll: &[usize; 3],
) {
    let permanents: Vec<&ObjectView> = view
        .battlefield
        .iter()
        .filter_map(|&slot| slots.get(slot))
        .filter(|object| side_of(object) == side)
        .collect();
    for (row, &offset) in ROWS.iter().zip(scroll) {
        let area = match row {
            RowName::Creatures => layout.creatures,
            RowName::Lands => layout.lands,
            RowName::Others => layout.others,
        };
        let mut cards: Vec<&ObjectView> = permanents
            .iter()
            .copied()
            .filter(|object| row_of(object) == *row)
            .collect();
        if *row == RowName::Lands {
            let names: Vec<Option<&str>> = cards.iter().map(|o| o.name.as_deref()).collect();
            let first = |object: &ObjectView| {
                names
                    .iter()
                    .position(|name| *name == object.name.as_deref())
                    .unwrap_or(0)
            };
            cards.sort_by_key(|object| (object.tapped, first(object)));
        }
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
        } in lay_row(&cards, area, y, &rules)
        {
            let place = Place::Card {
                zone: ZoneName::Battlefield,
                side,
                row: Some(*row),
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
}

/// The stack, raised above the table at its right: the bottom item first, each one above it
/// nearer the viewer and higher. Only spells are objects; abilities on the stack are drawn by
/// the flat layer.
fn stack(placement: &mut Placement, view: &PlayerView, slots: &Slots, layout: &Layout) {
    let spells = view.stack.iter().filter_map(|item| match item {
        StackItem::Spell { slot, .. } => slots.get(*slot),
        StackItem::Ability { .. } => None,
    });
    for (index, object) in spells.enumerate() {
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
