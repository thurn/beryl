//! Placement: for a view and a window, the pose every object rests in, by one function per zone
//! (docs/beryl-3d.md, "Objects and placement"). Each is pure: it reads the view, the layout, the
//! camera fitted to the window and the rows' scroll offsets, and nothing else.

use beryl_protocol::protocol::{
    CardType, ObjectId, ObjectView, PlayerState, PlayerView, RowName, Side, ZoneName,
};
use beryl_protocol::script::{Piles, Table as Played};
use glam::{Quat, Vec2, Vec3};

use super::rows::{Laid, NameIds, RowRules, SameName, face_up, lay_row};
use super::stack::{OnStack, on_stack, place_stack, spells};
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

/// Every object's resting place, by slot, and every ability's on the stack, by number.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Placement {
    /// Indexed by `ObjectId`; none for an object the table does not show (in a sideboard, or
    /// nowhere).
    pub rests: Vec<Option<Rest>>,
    /// The abilities on the stack, sorted by number.
    pub abilities: Vec<(u32, Rest)>,
}

impl Placement {
    #[must_use]
    pub fn get(&self, slot: ObjectId) -> Option<&Rest> {
        self.rests.get(usize::from(slot.0))?.as_ref()
    }

    /// Where the ability numbered `id` rests, if it is on the stack.
    #[must_use]
    pub fn ability(&self, id: u32) -> Option<&Rest> {
        let at = self
            .abilities
            .binary_search_by_key(&id, |(number, _)| *number)
            .ok()?;
        Some(&self.abilities[at].1)
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

/// Every object's resting place in `view`, seen through `camera`. `abilities` numbers the
/// abilities on the view's stack, bottom first, which the view does not name; an ability past
/// its end is not placed, though it keeps its place on the stack.
#[must_use]
pub fn place(
    view: &PlayerView,
    abilities: &[u32],
    layout: &Layout,
    camera: &Camera,
    scroll: &Scroll,
) -> Placement {
    place_zones(&Zones::of_view(view, abilities), layout, camera, scroll)
}

/// Every object's resting place in `zones`, seen through `camera`.
pub(crate) fn place_zones(
    zones: &Zones,
    layout: &Layout,
    camera: &Camera,
    scroll: &Scroll,
) -> Placement {
    let mut placement = Placement::default();
    for (group, objects) in Group::ALL.into_iter().zip(all_members(zones)) {
        let rules = (layout, camera, scroll, None);
        lay(group, &objects, &zones.stack, rules, &mut placement);
    }
    placement
}

/// Every object's resting place in `zones`, as `place_zones` gives it, taking a group's resting
/// places from `known`, the placement of `known_zones` under the same rules, wherever that group
/// holds equal objects in the same order in both. Comparing the objects costs less than laying
/// them out again.
pub(crate) fn place_reusing(
    zones: &Zones,
    (known_zones, known): (&Zones, &Placement),
    layout: &Layout,
    camera: &Camera,
    scroll: &Scroll,
) -> Placement {
    let mut placement = Placement {
        rests: Vec::with_capacity(known.rests.len()),
        abilities: Vec::new(),
    };
    let groups = Group::ALL.into_iter().zip(all_members(zones));
    for ((group, objects), was) in groups.zip(all_members(known_zones)) {
        let same = objects.len() == was.len()
            && objects.iter().zip(&was).all(|(a, b)| a == b)
            && (group != Group::Stack || zones.stack == known_zones.stack);
        if same {
            for object in objects {
                if let Some(rest) = known.get(object.slot) {
                    placement.set(object.slot, *rest);
                }
            }
            if group == Group::Stack {
                placement.abilities.clone_from(&known.abilities);
            }
        } else {
            lay(
                group,
                &objects,
                &zones.stack,
                (layout, camera, scroll, None),
                &mut placement,
            );
        }
    }
    placement
}

/// Places the objects of `group` in `zones` into `placement`, and leaves the slots it placed in
/// `placed`. Other groups' objects keep their places: no group's resting places depend on
/// another's objects.
pub(crate) fn place_group(
    zones: &Zones,
    group: Group,
    rules: Rules,
    placement: &mut Placement,
    placed: &mut Vec<ObjectId>,
) {
    let objects = members(zones, group);
    placed.clear();
    placed.extend(objects.iter().map(|object| object.slot));
    lay(group, &objects, &zones.stack, rules, placement);
}

/// What placement follows besides the objects: the layout, the camera, the rows' scroll offsets
/// and, if the caller keeps them, the numbers of the objects' names.
pub(crate) type Rules<'r> = (&'r Layout, &'r Camera, &'r Scroll, Option<&'r NameIds<'r>>);

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

    /// Whether it lays its objects out in the order the table lists every object, which a view
    /// and the table a script plays over list differently: by the view's order, and by slot.
    /// The other groups follow lists the two keep alike (the battlefield's, a library's or
    /// graveyard's) or, the stack, are placed again after every step.
    pub(crate) fn in_object_order(self) -> bool {
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
    pub(crate) fn at(place: Place) -> Option<Group> {
        let Place::Card { zone, side, row } = place else {
            return None;
        };
        Some(match (zone, row) {
            (ZoneName::Library | ZoneName::Graveyard | ZoneName::Exile, _) => {
                Group::Pile { side, zone }
            }
            (ZoneName::Hand, _) => Group::Hand(side),
            (ZoneName::Battlefield, Some(row)) => Group::Row { side, row },
            (ZoneName::Stack, _) => Group::Stack,
            _ => return None,
        })
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
    /// A view's objects, in the order its hands and exile piles list them; none for the table a
    /// script plays over, which lists them by slot, the order of `slots`.
    in_view_order: Option<&'a [ObjectView]>,
    /// Every object, by slot.
    slots: Vec<Option<&'a ObjectView>>,
    /// Permanents, in the order they entered.
    battlefield: &'a [ObjectId],
    /// The spells and numbered abilities on the stack, bottom first.
    stack: Vec<OnStack>,
    /// Each side's library and graveyard, bottom first: the viewer's and then the opponent's.
    piles: [[&'a [ObjectId]; 2]; 2],
}

impl<'a> Zones<'a> {
    /// The zones of `view`, whose stack's abilities `abilities` numbers, bottom first.
    pub(crate) fn of_view(view: &'a PlayerView, abilities: &[u32]) -> Self {
        let player = |player: &'a PlayerState| [&player.library[..], &player.graveyard[..]];
        let end = view
            .objects
            .iter()
            .map(|object| usize::from(object.slot.0) + 1)
            .max()
            .unwrap_or(0);
        Self {
            in_view_order: Some(&view.objects),
            slots: by_slot(end, view.objects.iter()),
            battlefield: &view.battlefield,
            stack: on_stack(view.stack.iter().map(|item| (item, None)), abilities),
            piles: [player(&view.players.you), player(&view.players.opponent)],
        }
    }

    /// The zones of the table a script plays over. It keeps its objects by slot, which is the
    /// order its hands and exile piles list them in.
    pub(crate) fn of_table(table: &'a Played) -> Self {
        let piles = |piles: &'a Piles| [&piles.library[..], &piles.graveyard[..]];
        let end = table
            .objects
            .last_key_value()
            .map_or(0, |(slot, _)| usize::from(slot.0) + 1);
        Self {
            in_view_order: None,
            slots: by_slot(end, table.objects.values()),
            battlefield: &table.battlefield,
            stack: on_stack(table.stack.iter().map(|entry| (&entry.item, entry.id)), &[]),
            piles: [piles(&table.you), piles(&table.opponent)],
        }
    }

    /// Every object, in the order the table lists them.
    fn objects(&self) -> impl Iterator<Item = &'a ObjectView> + '_ {
        let (in_view, by_slot) = match self.in_view_order {
            Some(objects) => (Some(objects.iter()), None),
            None => (None, Some(self.slots.iter().filter_map(|object| *object))),
        };
        in_view
            .into_iter()
            .flatten()
            .chain(by_slot.into_iter().flatten())
    }

    /// The object in `slot`, if the table holds one there.
    pub(crate) fn get(&self, slot: ObjectId) -> Option<&'a ObjectView> {
        self.slots.get(usize::from(slot.0)).copied().flatten()
    }

    /// The objects in one side's `zone`, in the order the table lists them.
    fn in_zone(&self, side: Side, zone: ZoneName) -> Vec<&'a ObjectView> {
        self.objects()
            .filter(|object| object.zone == zone && side_of(object) == side)
            .collect()
    }

    /// The objects in `slots`, in that order, leaving out any slot the table holds nothing in.
    fn listed(&self, slots: &[ObjectId]) -> Vec<&'a ObjectView> {
        slots.iter().filter_map(|&slot| self.get(slot)).collect()
    }
}

/// `objects`, none of whose slots is `end` or more, by slot.
fn by_slot<'a>(
    end: usize,
    objects: impl Iterator<Item = &'a ObjectView>,
) -> Vec<Option<&'a ObjectView>> {
    let mut slots = vec![None; end];
    for object in objects {
        slots[usize::from(object.slot.0)] = Some(object);
    }
    slots
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
        Group::Row { side, row } => {
            let mut found = Vec::with_capacity(zones.battlefield.len());
            found.extend(
                zones
                    .battlefield
                    .iter()
                    .filter_map(|&slot| zones.get(slot))
                    .filter(|object| side_of(object) == side && row_of(object) == row),
            );
            found
        }
        Group::Stack => zones.listed(&spells(&zones.stack)),
    }
}

/// The objects of every group, by `Group::index`, each in the order `members` gives, reading each
/// list of `zones` once rather than once for each group.
fn all_members<'a>(zones: &Zones<'a>) -> Vec<Vec<&'a ObjectView>> {
    let mut all: Vec<Vec<&'a ObjectView>> = Group::ALL.iter().map(|_| Vec::new()).collect();
    for side in [Side::You, Side::Opponent] {
        let piles = zones.piles[side_index(side)];
        for (zone, slots) in [ZoneName::Library, ZoneName::Graveyard]
            .into_iter()
            .zip(piles)
        {
            all[Group::Pile { side, zone }.index()] = zones.listed(slots);
        }
    }
    for object in zones.objects() {
        if matches!(object.zone, ZoneName::Hand | ZoneName::Exile)
            && let Some(group) = Group::of(object)
        {
            all[group.index()].push(object);
        }
    }
    for object in zones.battlefield.iter().filter_map(|&slot| zones.get(slot)) {
        let group = Group::Row {
            side: side_of(object),
            row: row_of(object),
        };
        all[group.index()].push(object);
    }
    all[Group::Stack.index()] = zones.listed(&spells(&zones.stack));
    all
}

/// Places `objects`, the members of `group`, into `placement`. The stack's group places every
/// item of `stack`.
fn lay(
    group: Group,
    objects: &[&ObjectView],
    stack: &[OnStack],
    rules: Rules,
    placement: &mut Placement,
) {
    let (layout, camera, scroll, ids) = rules;
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
            battlefield_row(
                placement,
                objects,
                (side, row),
                (layout, camera),
                offset,
                ids,
            );
        }
        Group::Stack => place_stack(placement, objects, stack, layout),
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
pub(super) fn turn(object: &ObjectView) -> f32 {
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
    (side, row): (Side, RowName),
    (layout, camera): (&Layout, &Camera),
    offset: usize,
    ids: Option<&NameIds<'_>>,
) {
    let area = match row {
        RowName::Creatures => layout.creatures,
        RowName::Lands => layout.lands,
        RowName::Others => layout.others,
    };
    let sorted;
    let (cards, names) = if row == RowName::Lands {
        let names;
        (sorted, names) = lands_in_order(cards, ids);
        (&sorted[..], Some(names))
    } else {
        (cards, None)
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
    let place = Place::Card {
        zone: ZoneName::Battlefield,
        side,
        row: Some(row),
    };
    let mut set = |Laid {
                       object,
                       pose,
                       visible,
                   }| {
        placement.set(
            object.slot,
            Rest {
                pose,
                place,
                visible,
            },
        );
    };
    lay_row(cards, (names, ids), area, y, &rules, &mut set);
}

/// A lands row's cards in the order they stand: untapped first, and each name together, in the
/// order the names first appear in `cards`. Otherwise the order of `cards` is kept. Also links
/// each card, in that order, to the one before it with its name.
fn lands_in_order<'a>(
    cards: &[&'a ObjectView],
    ids: Option<&NameIds<'_>>,
) -> (Vec<&'a ObjectView>, SameName) {
    let names = SameName::of(cards, ids);
    // The index of the first card with each card's name, and the next card after each with the
    // same name.
    let mut first = Vec::with_capacity(cards.len());
    let mut after: Vec<Option<usize>> = vec![None; cards.len()];
    for index in 0..cards.len() {
        first.push(names.first(index, &first));
        if let Some(previous) = names.before(index) {
            after[previous] = Some(index);
        }
    }
    // Each name's cards, untapped and then tapped, from its first card. `latest` holds where
    // the card last put in order with each name went, by the index of that name's first card.
    let mut order = Vec::with_capacity(cards.len());
    let mut previous = Vec::with_capacity(cards.len());
    let mut latest: Vec<Option<usize>> = vec![None; cards.len()];
    for tapped in [false, true] {
        for (index, &first) in first.iter().enumerate() {
            if first != index {
                continue;
            }
            let mut next = Some(index);
            while let Some(card) = next {
                if cards[card].tapped == tapped {
                    previous.push(latest[index]);
                    latest[index] = Some(order.len());
                    order.push(cards[card]);
                }
                next = after[card];
            }
        }
    }
    (order, SameName::from_previous(previous))
}
