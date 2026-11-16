//! Placement: for a view and a window, the pose every object rests in, by one function per zone
//! (docs/beryl-3d.md, "Objects and placement"). Each is pure: it reads the view, the layout, the
//! camera fitted to the window and the rows' scroll offsets, and nothing else.

use std::borrow::Cow;

use beryl_protocol::protocol::{
    CardType, ObjectId, ObjectView, PlayerState, PlayerView, RowName, Side, StackItem, ZoneName,
};
use beryl_protocol::script::{Piles, Table as Played};

mod indexed;
pub(crate) use indexed::arrival;
mod under;
pub(crate) use under::host_slot;

use super::hands::hand;
use super::pile::{PileShown, lay_pile, pile_base, pile_scale};
use super::rows::{
    Beneath, Laid, NameIds, RowRules, RowWindow, SameName, face_up, lay_row, least_strip,
};
use super::stack::{OnStack, on_stack, place_stack, row_area, spells};
use super::{Camera, Layout, lifted};
use crate::agent::hit;
use crate::frame::Place;
use crate::pose::Pose;

/// Where one object rests.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rest {
    pub pose: Pose,
    pub place: Place,
    /// False for a card scrolled out of its row's view, which is not drawn.
    pub visible: bool,
    /// How many identical permanents the top card of a pile of them stands for, which it wears
    /// as a count; 0 for each copy beneath a pile's top, which a press reaches through the top
    /// card as the web interface's does; and 1 for every other card.
    pub copies: u16,
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
    /// Each library, graveyard and exile pile, by `PileShown::index`.
    pub piles: [PileShown; 6],
    /// Which groups of each battlefield row are in view, as `Scroll` orders the rows.
    pub rows: Rows,
}

/// Which groups of each battlefield row are in view: for the viewer and then the opponent, each
/// in the order of `ROWS`.
pub type Rows = [[RowWindow; 3]; 2];

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

    /// How many objects of `rests` have a resting place. The abilities on the stack are left
    /// out, as `Work::placed` leaves them out.
    pub(crate) fn count(&self) -> usize {
        self.rests.iter().flatten().count()
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
        lay(
            group,
            &objects,
            &zones.stack,
            (rules, zones.handling.as_ref()),
            &mut placement,
        );
    }
    placement
}

/// Every object's resting place in `zones`, as `place_zones` gives it, taking a group's resting
/// places from `known`, the placement of `known_zones` under the same rules, wherever that group
/// holds equal objects in the same order in both. Comparing the objects costs less than laying
/// them out again. Returns it with how many objects it laid out rather than copied.
pub(crate) fn place_reusing(
    zones: &Zones,
    (known_zones, known): (&Zones, &Placement),
    layout: &Layout,
    camera: &Camera,
    scroll: &Scroll,
) -> (Placement, usize) {
    let mut placement = Placement {
        rests: Vec::with_capacity(known.rests.len()),
        abilities: Vec::new(),
        piles: known.piles,
        rows: known.rows,
    };
    let mut laid = 0;
    let groups = Group::ALL.into_iter().zip(all_members(zones));
    for ((group, objects), was) in groups.zip(all_members(known_zones)) {
        let same = objects.len() == was.len()
            && objects.iter().zip(&was).all(|(a, b)| a == b)
            && (group != Group::Stack || zones.stack == known_zones.stack);
        // A group the cards being handled are lifted from is placed again when the cards
        // handled changed, though its objects did not.
        let lifted_from = |handled: Option<&lifted::Handled>| {
            handled.and_then(|handled| lifted::group_of(handled.handling)) == Some(group)
        };
        let (handled, known_handled) = (zones.handling.as_ref(), known_zones.handling.as_ref());
        let same = same
            && (handled == known_handled || !(lifted_from(handled) || lifted_from(known_handled)));
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
            laid += objects.len();
            lay(
                group,
                &objects,
                &zones.stack,
                ((layout, camera, scroll, None), zones.handling.as_ref()),
                &mut placement,
            );
        }
    }
    (placement, laid)
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
    lay(
        group,
        &objects,
        &zones.stack,
        (rules, zones.handling.as_ref()),
        placement,
    );
}

/// What placement follows besides the objects: the layout, the camera, the rows' scroll offsets
/// and, if the caller keeps them, the numbers of the objects' names.
pub(crate) type Rules<'r> = (&'r Layout, &'r Camera, &'r Scroll, Option<&'r NameIds<'r>>);

/// What `Zones::of_table_grouped` holds for a slot with no object, or one the table does not
/// show: no `Group::number`.
pub(crate) const NO_GROUP: u8 = u8::MAX;

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

    /// `index`, as one byte.
    pub(crate) fn number(self) -> u8 {
        u8::try_from(self.index()).expect("fewer than 256 groups")
    }

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
            // A card in exile that lies under a permanent rests in that permanent's row.
            (ZoneName::Battlefield | ZoneName::Merged | ZoneName::Exile, Some(row)) => {
                Group::Row { side, row }
            }
            (ZoneName::Library | ZoneName::Graveyard | ZoneName::Exile, _) => {
                Group::Pile { side, zone }
            }
            (ZoneName::Hand, _) => Group::Hand(side),
            (ZoneName::Stack, _) => Group::Stack,
            _ => return None,
        })
    }

    /// The group `object` belongs in, by its zone, if the table shows it. A card that lies under
    /// a permanent (a merged permanent's, or one it holds in exile) belongs in that permanent's
    /// row, which only `Zones::group_of` can find.
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
pub(super) fn row_index(row: RowName) -> usize {
    ROWS.iter()
        .position(|other| *other == row)
        .expect("every row is in `ROWS`")
}

/// `side`'s place in `Scroll` and `Zones::piles`: the viewer first.
pub(super) fn side_index(side: Side) -> usize {
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
    slots: Cow<'a, [Option<&'a ObjectView>]>,
    /// The `Group::number` of the object in each slot, or `NO_GROUP`, where the caller keeps
    /// them (`of_table_grouped`).
    groups: Option<&'a [u8]>,
    /// Permanents, in the order they entered.
    battlefield: &'a [ObjectId],
    /// The spells and numbered abilities on the stack, bottom first.
    stack: Cow<'a, [OnStack]>,
    /// Each side's library and graveyard, bottom first: the viewer's and then the opponent's.
    piles: [[&'a [ObjectId]; 2]; 2],
    /// The cards the opponent is handling, which rest lifted off their pile (`lifted`).
    pub(crate) handling: Option<lifted::Handled>,
    /// For the table a script plays over, the order in which its hands and exile piles list
    /// their cards, by slot (`arrived`); none to list them by slot.
    arrival: Option<&'a [u32]>,
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
        let slots = by_slot(end, view.objects.iter());
        // A spell whose card has left the stack is not on it, though the view still lists it
        // while its resolution goes on: a spell that exiles itself as it resolves and then
        // asks a player to choose. The table a script plays over leaves it out the same way
        // (`Table::of`), so its card rests in the zone it went to, not on the stack.
        let on = |item: &StackItem| match item {
            StackItem::Spell { slot, .. } => slots
                .get(usize::from(slot.0))
                .copied()
                .flatten()
                .is_some_and(|object| object.zone == ZoneName::Stack),
            StackItem::Ability { .. } => true,
        };
        let stack = on_stack(
            view.stack
                .iter()
                .filter(|item| on(item))
                .map(|item| (item, None)),
            abilities,
            view.objects.iter(),
        );
        let zones = Self {
            in_view_order: Some(&view.objects),
            slots: Cow::Owned(slots),
            groups: None,
            battlefield: &view.battlefield,
            stack: Cow::Owned(stack),
            piles: [player(&view.players.you), player(&view.players.opponent)],
            handling: None,
            arrival: None,
        };
        let handled = lifted::Handled::of_view(&zones, view.handling);
        zones.handled(handled)
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
            slots: Cow::Owned(by_slot(end, table.objects.values())),
            groups: None,
            battlefield: &table.battlefield,
            stack: Cow::Owned(on_stack(
                table.stack.iter().map(|entry| (&entry.item, entry.id)),
                &[],
                table.objects.values(),
            )),
            piles: [piles(&table.you), piles(&table.opponent)],
            handling: None,
            arrival: None,
        }
    }

    /// The same zones, with the cards `handled` names being handled. The table a script plays
    /// over does not keep which cards are handled, so the script's player keeps it.
    pub(crate) fn handled(self, handled: Option<lifted::Handled>) -> Self {
        Self {
            handling: handled,
            ..self
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

    /// The objects in one side's `zone`, in the order the table lists them, but for the cards
    /// in exile that lie under a permanent, in its row (`host`).
    fn in_zone(&self, side: Side, zone: ZoneName) -> Vec<&'a ObjectView> {
        let mut found: Vec<&'a ObjectView> = self
            .objects()
            .filter(|object| object.zone == zone && side_of(object) == side)
            .filter(|object| zone != ZoneName::Exile || self.host(object).is_none())
            .collect();
        self.in_arrival_order(&mut found);
        found
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
pub(crate) fn members<'a>(zones: &Zones<'a>, group: Group) -> Vec<&'a ObjectView> {
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
            let mut cards = zones.in_row(side, row);
            let under = zones.under(&cards);
            cards.extend(under);
            cards
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
    // The cards that lie under permanents, each put after the permanents of its row.
    let mut under = Vec::new();
    for object in zones.objects() {
        let held = matches!(object.zone, ZoneName::Merged | ZoneName::Exile)
            && zones.host(object).is_some();
        if held {
            under.push(object);
        } else if matches!(object.zone, ZoneName::Hand | ZoneName::Exile)
            && let Some(group) = Group::of(object)
        {
            all[group.index()].push(object);
        }
    }
    for object in zones.battlefield.iter().filter_map(|&slot| zones.get(slot)) {
        if zones.host(object).is_some() {
            under.push(object);
            continue;
        }
        let group = Group::Row {
            side: side_of(object),
            row: row_of(object),
        };
        let row = &mut all[group.index()];
        // Room for the whole battlefield as a row takes its first card, so that no row grows one
        // copy at a time and a row with no cards allocates nothing.
        if row.capacity() == 0 {
            row.reserve_exact(zones.battlefield.len());
        }
        row.push(object);
    }
    for object in under {
        if let Some(group) = zones.group_of(object) {
            all[group.index()].push(object);
        }
    }
    for group in Group::ALL
        .into_iter()
        .filter(|group| group.in_object_order())
    {
        zones.in_arrival_order(&mut all[group.index()]);
    }
    all[Group::Stack.index()] = zones.listed(&spells(&zones.stack));
    all
}

/// Places `objects`, the members of `group`, into `placement`, lifting the cards `handled`
/// names from it. The stack's group places every item of `stack`.
fn lay(
    group: Group,
    objects: &[&ObjectView],
    stack: &[OnStack],
    (rules, handled): (Rules, Option<&lifted::Handled>),
    placement: &mut Placement,
) {
    let (layout, camera, scroll, ids) = rules;
    match group {
        Group::Pile { side, zone } => {
            let at = pile_base(layout, camera, side, zone);
            lay_pile(
                placement,
                objects,
                (side, zone),
                (at, pile_scale(layout, side)),
            );
        }
        Group::Hand(side) => {
            let data = match side {
                Side::You => layout.hand,
                Side::Opponent => layout.opponent_hand,
            };
            hand(placement, objects, side, (data, layout.strip), camera);
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
        Group::Stack => place_stack(placement, objects, stack, (&layout.stack, camera)),
    }
    lifted::lay(handled, group, objects, (layout, camera), placement);
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

/// The turn of a card that shows its face (0) or its back (1).
pub(super) fn turn(object: &ObjectView) -> f32 {
    if face_up(object) { 0.0 } else { 1.0 }
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
    let (area, y) = row_area((layout, camera), side, row);
    // The cards that lie under permanents, which `members` lists after the row's permanents. A
    // row with none, as nearly every row is, is laid out with no copy of its list.
    let in_row = |slot: ObjectId| cards.iter().copied().find(|object| object.slot == slot);
    let lies_under = |object: &&ObjectView| host_slot(object, in_row).is_some();
    let (under, permanents): (Vec<&ObjectView>, Vec<&ObjectView>);
    let cards = if cards.iter().any(lies_under) {
        (under, permanents) = cards.iter().copied().partition(lies_under);
        &permanents[..]
    } else {
        under = Vec::new();
        cards
    };
    let sorted;
    let (cards, names) = if row == RowName::Lands {
        let names;
        (sorted, names) = lands_in_order(cards, ids);
        (&sorted[..], Some(names))
    } else {
        (cards, None)
    };
    let rules = RowRules {
        layout,
        // The "reachable" check finds an uncovered square only in whole cells of its grid, and
        // can miss up to a cell beside each of the strip's two slanted edges.
        strip: least_strip(camera, area, y, layout.strip + 2.0 * hit::CELL),
        scroll: offset,
    };
    let place = |zone| Place::Card {
        zone,
        side,
        row: Some(row),
    };
    let mut set = |Laid {
                       object,
                       pose,
                       visible,
                       copies,
                   }| {
        placement.set(
            object.slot,
            Rest {
                pose,
                place: place(object.zone),
                visible,
                copies,
            },
        );
    };
    let beneath = Beneath::new(under, cards);
    placement.rows[side_index(side)][row_index(row)] =
        lay_row((cards, &beneath), (names, ids), area, y, &rules, &mut set);
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
