//! Placement: for a view and a window, the pose every object rests in, by one function per zone
//! (docs/beryl-3d.md, "Objects and placement"). Each is pure: it reads the view, the layout, the
//! camera fitted to the window and the rows' scroll offsets, and nothing else.

use std::borrow::Cow;

use beryl_protocol::protocol::{
    ObjectId, ObjectView, PlayerState, PlayerView, RowName, Side, StackItem, ZoneName,
};
use beryl_protocol::script::{Piles, Table as Played};

mod indexed;
mod policy;
pub use indexed::arrival;
pub use policy::PileRules;
mod row;
pub(crate) use row::{RowFits, rezoom, zooms};
pub use row::{Zooms, blocked, blocking_rows, prepared as prepare_row};
use row::{battlefield_row, row_of};
mod under;
pub use under::{host_slot, names_host};

use super::hands::{HandFits, hand};
use super::pile::{PileShown, lay_pile, lay_unseen_exile};
use super::rows::{NameIds, RowWindow, face_up};
use super::stack::{OnStack, on_stack, place_stack, spells};
use super::{Camera, Layout, lifted};
use kit_base::data::verbs::Lift;
use kit_base::pose::Pose;
use kit_frame::frame::Place;

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
    /// Which groups of each battlefield row, and which cards of the viewer's hand and the
    /// stack, are in view.
    pub rows: Rows,
    /// How far each battlefield row is zoomed out.
    pub zooms: Zooms,
}

/// A line of cards that scrolls when it holds more than fit with their least strip: a
/// battlefield row, the viewer's hand or the stack (docs/beryl-3d.md, "Objects and placement").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scroller {
    Row(Side, RowName),
    /// The viewer's hand. The opponent's is not theirs to press, so it closes up instead.
    Hand,
    Stack,
}

/// Which groups of each battlefield row, and which cards of the viewer's hand and items of the
/// stack, are in view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rows {
    /// For the viewer and then the opponent, each in the order of `ROWS`.
    pub battlefield: [[RowWindow; 3]; 2],
    /// The viewer's hand, its cards in order from the left.
    pub hand: RowWindow,
    /// The stack, its items in order from the bottom.
    pub stack: RowWindow,
}

impl Rows {
    /// Which groups of `scroller` are in view.
    #[must_use]
    pub fn get(&self, scroller: Scroller) -> RowWindow {
        match scroller {
            Scroller::Row(side, row) => self.battlefield[side_index(side)][row_index(row)],
            Scroller::Hand => self.hand,
            Scroller::Stack => self.stack,
        }
    }
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

    /// How many objects of `rests` have a resting place. The abilities on the stack are left
    /// out, as `Work::placed` leaves them out.
    pub(crate) fn count(&self) -> usize {
        self.rests.iter().flatten().count()
    }

    /// Stores the resting place of the object at `slot`.
    pub fn set(&mut self, slot: ObjectId, rest: Rest) {
        let index = usize::from(slot.0);
        if self.rests.len() <= index {
            self.rests.resize(index + 1, None);
        }
        self.rests[index] = Some(rest);
    }
}

/// The scroll offset of each line of cards that scrolls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Scroll {
    /// Each battlefield row's, in groups from its left end: for the viewer and then the
    /// opponent, each in the order of `ROWS`.
    pub battlefield: [[usize; 3]; 2],
    /// The viewer's hand's, in cards from its left end.
    pub hand: usize,
    /// The stack's, in items from its top, so that an item cast while the stack is scrolled to
    /// its top comes into view.
    pub stack: usize,
}

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
    lift: &Lift,
) -> Placement {
    let fits = RowFits::new(layout, camera);
    place_zones(
        &Zones::of_view(view, abilities),
        (layout, camera, scroll, &fits, None, lift),
    )
}

/// Every object's resting place in `zones`, seen through `camera`.
pub(crate) fn place_zones(zones: &Zones, rules: Rules<'_>) -> Placement {
    let piles = zones.pile_rules();
    let mut placement = Placement {
        zooms: zooms(zones, &piles, rules),
        ..Placement::default()
    };
    for (group, objects) in Group::ALL.into_iter().zip(zone_members(zones)) {
        lay(
            group,
            &objects,
            (zones, &piles, rules, None),
            &mut placement,
        );
    }
    row::lay_blockers_again((zones, &piles, rules), &mut placement);
    placement
}

/// Every object's resting place in `zones`, as `place_zones` gives it, taking a group's resting
/// places from `known`, the placement of `known_zones` under the same rules, wherever that group
/// holds equal objects in the same order in both. Comparing the objects costs less than laying
/// them out again. Returns it with how many objects it laid out rather than copied.
pub(crate) fn place_reusing(
    zones: &Zones,
    (known_zones, known): (&Zones, &Placement),
    rules: Rules<'_>,
) -> (Placement, usize) {
    let mut placement = Placement {
        rests: Vec::with_capacity(known.rests.len()),
        abilities: Vec::new(),
        piles: known.piles,
        rows: known.rows,
        zooms: Zooms::default(),
    };
    let mut laid = 0;
    let (piles, known_piles) = (zones.pile_rules(), known_zones.pile_rules());
    placement.zooms = zooms(zones, &piles, rules);
    let zoomed = placement.zooms != zooms(known_zones, &known_piles, rules);
    let groups = Group::ALL.into_iter().zip(zone_members(zones));
    for ((group, objects), was) in groups.zip(zone_members(known_zones)) {
        let same = objects.len() == was.len()
            && objects.iter().zip(&was).all(|(a, b)| a == b)
            && (group != Group::Stack || zones.stack == known_zones.stack)
            && piles.same_for_row(&known_piles, group, &objects)
            && !(zoomed && matches!(group, Group::Row { .. }));
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
                (zones, &piles, rules, None),
                &mut placement,
            );
        }
    }
    laid += row::lay_blockers_again((zones, &piles, rules), &mut placement);
    (placement, laid)
}

/// Places the objects of `group` in `zones` into `placement`, and leaves the slots it placed in
/// `placed`. The caller places each group affected by the current pile rules.
pub(crate) fn place_group(
    (zones, piles): (&Zones, &PileRules),
    group: Group,
    rules: Rules,
    (placement, hands): (&mut Placement, &mut HandFits),
    placed: &mut Vec<ObjectId>,
) {
    let objects = members(zones, group);
    placed.clear();
    placed.extend(objects.iter().map(|object| object.slot));
    lay(
        group,
        &objects,
        (zones, piles, rules, Some(hands)),
        placement,
    );
}

/// What placement follows besides the objects: the layout, the camera, the rows' scroll offsets
/// and fitted geometry, and the numbers of the objects' names if the caller keeps them.
pub(crate) type Rules<'r> = (
    &'r Layout,
    &'r Camera,
    &'r Scroll,
    &'r RowFits,
    Option<&'r NameIds<'r>>,
    &'r Lift,
);

/// What `Zones::of_table_grouped` holds for a slot with no object, or one the table does not
/// show: no `Group::number`.
pub const NO_GROUP: u8 = u8::MAX;

/// A part of the table whose objects are placed together and apart from every other part: a
/// pile, a hand, a battlefield row or the stack. Rows also read the pile rules, since the
/// stack names some permanents and lands in other rows count toward the land threshold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
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
    pub const ALL: [Group; 15] = {
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
    pub fn number(self) -> u8 {
        u8::try_from(self.index()).expect("fewer than 256 groups")
    }

    /// Its place in `ALL`.
    pub fn index(self) -> usize {
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
    pub fn of(object: &ObjectView) -> Option<Group> {
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
pub struct Zones<'a> {
    /// A view's objects, in the order its hands and exile piles list them; none for the table a
    /// script plays over, which lists them by slot, the order of `slots`.
    in_view_order: Option<&'a [ObjectView]>,
    /// Every object, by slot.
    slots: Cow<'a, [Option<&'a ObjectView>]>,
    /// The `Group::number` of the object in each slot, or `NO_GROUP`, where the caller keeps
    /// them (`of_table_grouped`).
    groups: Option<&'a [u8]>,
    /// Slots of cards with a possible host, when the script indexes them.
    tucked: Option<&'a [ObjectId]>,
    /// Hand and exile slots, by group, where replay maintains them between steps.
    object_order: Option<&'a [Vec<ObjectId>; Group::ALL.len()]>,
    /// Permanents, in the order they entered.
    battlefield: &'a [ObjectId],
    /// The spells and numbered abilities on the stack, bottom first.
    stack: Cow<'a, [OnStack]>,
    /// The stack items that may name a battlefield permanent.
    stack_items: policy::StackItems<'a>,
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
    pub fn of_view(view: &'a PlayerView, abilities: &[u32]) -> Self {
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
            stack_slots(view.objects.iter()),
        );
        let zones = Self {
            in_view_order: Some(&view.objects),
            slots: Cow::Owned(slots),
            groups: None,
            tucked: None,
            object_order: None,
            battlefield: &view.battlefield,
            stack: Cow::Owned(stack),
            stack_items: policy::StackItems::View(&view.stack),
            piles: [player(&view.players.you), player(&view.players.opponent)],
            handling: None,
            arrival: None,
        };
        let handled = lifted::Handled::of_view(&zones, view.handling, view.turn.number == 0);
        zones.handled(handled)
    }

    /// The zones of the table a script plays over. It keeps its objects by slot, which is the
    /// order its hands and exile piles list them in.
    pub fn of_table(table: &'a Played) -> Self {
        let piles = |piles: &'a Piles| [&piles.library[..], &piles.graveyard[..]];
        let end = table
            .objects
            .last_key_value()
            .map_or(0, |(slot, _)| usize::from(slot.0) + 1);
        Self {
            in_view_order: None,
            slots: Cow::Owned(by_slot(end, table.objects.values())),
            groups: None,
            tucked: None,
            object_order: None,
            battlefield: &table.battlefield,
            stack: Cow::Owned(on_stack(
                table.stack.iter().map(|entry| (&entry.item, entry.id)),
                &[],
                stack_slots(table.objects.values()),
            )),
            stack_items: policy::StackItems::Played(&table.stack),
            piles: [piles(&table.you), piles(&table.opponent)],
            handling: None,
            arrival: None,
        }
    }

    /// The same zones, with the cards `handled` names being handled. The table a script plays
    /// over does not keep which cards are handled, so the script's player keeps it.
    #[must_use]
    pub fn handled(self, handled: Option<lifted::Handled>) -> Self {
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
    /// The object at `slot`, if it is present in the zones.
    pub fn get(&self, slot: ObjectId) -> Option<&'a ObjectView> {
        self.slots.get(usize::from(slot.0)).copied().flatten()
    }

    /// The objects in one side's `zone`, in the order the table lists them, but for the cards
    /// in exile that lie under a permanent, in its row (`host`).
    fn in_zone(&self, side: Side, zone: ZoneName) -> Vec<&'a ObjectView> {
        let mut found: Vec<&'a ObjectView> = if let Some(order) = self.object_order {
            let group = if zone == ZoneName::Hand {
                Group::Hand(side)
            } else {
                Group::Pile { side, zone }
            };
            order[group.index()]
                .iter()
                .filter_map(|&slot| self.get(slot))
                .filter(|object| zone != ZoneName::Exile || self.host(object).is_none())
                .collect()
        } else {
            self.objects()
                .filter(|object| object.zone == zone && side_of(object) == side)
                .filter(|object| zone != ZoneName::Exile || self.host(object).is_none())
                .collect()
        };
        self.in_arrival_order(&mut found);
        found
    }

    /// The objects in `slots`, in that order, leaving out any slot the table holds nothing in.
    fn listed(&self, slots: &[ObjectId]) -> Vec<&'a ObjectView> {
        slots.iter().filter_map(|&slot| self.get(slot)).collect()
    }
}

/// The slots of cards in the stack zone, in the order the caller lists them.
pub fn stack_slots<'a>(
    objects: impl Iterator<Item = &'a ObjectView>,
) -> impl Iterator<Item = ObjectId> {
    objects
        .filter(|object| object.zone == ZoneName::Stack)
        .map(|object| object.slot)
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
pub fn zone_members<'a>(zones: &Zones<'a>) -> Vec<Vec<&'a ObjectView>> {
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
    (zones, piles, rules, hands): (&Zones, &PileRules, Rules, Option<&mut HandFits>),
    placement: &mut Placement,
) {
    let (layout, camera, scroll, fits, ids, lift) = rules;
    match group {
        Group::Pile { side, zone } => {
            if zone == ZoneName::Exile && layout.phone.is_some() {
                lay_unseen_exile(placement, objects, side, (layout, camera));
            } else {
                lay_pile(placement, objects, (side, zone), (layout, camera));
            }
        }
        Group::Hand(side) => {
            let data = match side {
                Side::You => layout.hand,
                Side::Opponent => layout.opponent_hand,
            };
            hand(
                placement,
                objects,
                (side, scroll.hand),
                (data, layout.strip, &lift.hand),
                camera,
                hands,
            );
        }
        Group::Row { side, row } => {
            let offset = scroll.battlefield[side_index(side)][row_index(row)];
            battlefield_row(
                placement,
                objects,
                (side, row),
                (layout, fits.get(side, row), camera),
                offset,
                (ids, piles.for_group(group)),
            );
        }
        Group::Stack => place_stack(
            placement,
            objects,
            &zones.stack,
            (layout, camera, scroll.stack),
        ),
    }
    lifted::lay(
        zones.handling.as_ref(),
        group,
        objects,
        (layout, camera, lift),
        placement,
    );
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
