//! Placement: for a view and a window, the pose every object rests in, by one function per zone
//! (docs/beryl-3d.md, "Objects and placement"). Each is pure: it reads the view, the layout, the
//! camera fitted to the window and the rows' scroll offsets, and nothing else.

use std::borrow::Cow;

use beryl_protocol::protocol::{
    ObjectId, ObjectView, PlayerState, PlayerView, RowName, Side, StackItem, ZoneName,
};
use beryl_protocol::script::{Piles, Table as Played};

mod group;
mod indexed;
mod policy;
mod probe;
pub use group::{Group, NO_GROUP};
pub use indexed::arrival;
pub use policy::PileRules;
pub use probe::ContainerDecision;
pub(crate) use probe::Decisions;
mod row;
use row::battlefield_row;
pub use row::{RowDecision, Zooms, blocked, blocking_rows, prepared as prepare_row, row_of};
pub(crate) use row::{RowFits, rezoom, zooms};
mod castable;
use castable::graveyard_pile;
pub use castable::in_hand_fan;

mod under;
pub use under::{host_slot, names_host};

use super::RowWindow;
use super::hands::{HandFits, hand};
use super::pile::{PILE_SLOTS, PileShown, lay_pile, lay_unseen_exile};
use super::rows::{NameIds, face_up};
use super::stack::{OnStack, on_stack, place_stack, spells};
use super::{Camera, Layout, Place, TableShape, lifted};
use kit_base::data::verbs::Lift;
use kit_base::pose::Pose;

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
    /// Whether its battlefield row lays it partly over or under another card on purpose
    /// (`solver::Placed::overlapped`). Every other card is false.
    pub overlapped: bool,
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
    pub piles: [PileShown; PILE_SLOTS],
    /// Which groups of each battlefield row, and which cards of the viewer's hand and the
    /// stack, are in view.
    pub rows: Rows,
    /// How far each battlefield row is zoomed out.
    pub zooms: Zooms,
    /// Present only for an inspection solve.
    pub(crate) decisions: Option<Box<Decisions>>,
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
    let mut layout = *layout;
    layout.shape = TableShape::of_view(view);
    let fits = RowFits::new(&layout, camera);
    place_zones(
        &Zones::of_view(view, abilities),
        (&layout, camera, scroll, &fits, None, lift),
    )
}

/// Every object's resting place in `zones`, seen through `camera`.
pub(crate) fn place_zones(zones: &Zones, rules: Rules<'_>) -> Placement {
    solve_zones(zones, rules, None)
}

pub(super) fn solve_zones(
    zones: &Zones,
    rules: Rules<'_>,
    decisions: Option<Box<Decisions>>,
) -> Placement {
    let piles = zones.pile_rules();
    let mut placement = Placement {
        zooms: zooms(zones, &piles, rules),
        decisions,
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
        decisions: None,
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
            handled.and_then(lifted::Handled::group) == Some(group)
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
    /// Whether this view or played table uses the shared library, graveyard and exile piles.
    pub(crate) shared: bool,
    /// Each side's library and graveyard, bottom first: the viewer's and then the opponent's.
    piles: [[&'a [ObjectId]; 2]; 2],
    /// The shared library and graveyard, bottom first, when `shared` is true.
    shared_piles: [&'a [ObjectId]; 2],
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
            view.stack_with_resolving()
                .filter(|item| on(item))
                .map(|item| (item, None)),
            abilities,
            stack_slots(view.objects.iter()),
        );
        let (shared, piles, shared_piles) = match &view.shared {
            Some(shared) => (
                true,
                [[&[][..], &[][..]], [&[][..], &[][..]]],
                [&shared.library[..], &shared.graveyard[..]],
            ),
            None => (
                false,
                [player(&view.players.you), player(&view.players.opponent)],
                [&[][..], &[][..]],
            ),
        };
        let zones = Self {
            in_view_order: Some(&view.objects),
            slots: Cow::Owned(slots),
            groups: None,
            tucked: None,
            object_order: None,
            battlefield: &view.battlefield,
            stack: Cow::Owned(stack),
            stack_items: policy::StackItems::View(view),
            shared,
            piles,
            shared_piles,
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
        let (shared, piles, shared_piles) = match &table.shared {
            Some(shared) => (
                true,
                [[&[][..], &[][..]], [&[][..], &[][..]]],
                piles(shared),
            ),
            None => (
                false,
                [piles(&table.you), piles(&table.opponent)],
                [&[][..], &[][..]],
            ),
        };
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
            shared,
            piles,
            shared_piles,
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

    /// The objects in one side's `zone` (or the shared exile pile when `side` is `None`), in
    /// the order the table lists them, but for the cards in exile that lie under a permanent,
    /// in its row (`host`).
    fn in_zone(&self, side: Option<Side>, zone: ZoneName) -> Vec<&'a ObjectView> {
        let mut found: Vec<&'a ObjectView> = if let Some(order) = self.object_order {
            let group = match (zone, side) {
                (ZoneName::Hand, Some(side)) => Group::Hand(side),
                _ => Group::Pile { side, zone },
            };
            // The hand's group also orders the viewer's graveyard cards it shows (`in_hand_fan`),
            // which `members` adds from the graveyard itself, so only the zone's own are kept.
            order[group.index()]
                .iter()
                .filter_map(|&slot| self.get(slot))
                .filter(|object| object.zone == zone)
                .filter(|object| zone != ZoneName::Exile || self.host(object).is_none())
                .collect()
        } else {
            self.objects()
                .filter(|object| {
                    object.zone == zone && side.is_none_or(|side| side_of(object) == side)
                })
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
        Group::Pile { side, .. } if side.is_none() != zones.shared => Vec::new(),
        Group::Pile {
            side: Some(side),
            zone: ZoneName::Library,
        } => zones.listed(zones.piles[side_index(side)][0]),
        Group::Pile {
            side: None,
            zone: ZoneName::Library,
        } => zones.listed(zones.shared_piles[0]),
        Group::Pile {
            side,
            zone: ZoneName::Graveyard,
        } => graveyard_pile(zones, side).0,
        Group::Pile { side, zone } => zones.in_zone(side, zone),
        Group::Hand(side) => {
            let mut hand = zones.in_zone(Some(side), ZoneName::Hand);
            if zones.shared {
                if side == Side::You {
                    hand.extend(graveyard_pile(zones, None).1);
                }
            } else {
                hand.extend(graveyard_pile(zones, Some(side)).1);
            }
            hand
        }
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
    // The viewer's graveyard cards they may cast from there, which the hand lists after its
    // own cards once those are in order.
    let mut castable = Vec::new();
    if zones.shared {
        let [library, graveyard] = [ZoneName::Library, ZoneName::Graveyard]
            .map(|zone| Group::Pile { side: None, zone }.index());
        all[library] = zones.listed(zones.shared_piles[0]);
        let fan;
        (all[graveyard], fan) = graveyard_pile(zones, None);
        castable.extend(fan);
    } else {
        for side in [Side::You, Side::Opponent] {
            let [library, graveyard] = [ZoneName::Library, ZoneName::Graveyard].map(|zone| {
                Group::Pile {
                    side: Some(side),
                    zone,
                }
                .index()
            });
            all[library] = zones.listed(zones.piles[side_index(side)][0]);
            let fan;
            (all[graveyard], fan) = graveyard_pile(zones, Some(side));
            castable.extend(fan);
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
            && let Some(group) = Group::of_in(object, zones.shared)
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
    all[Group::Hand(Side::You).index()].extend(castable);
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
            // The zones, not the layout, say whether the piles are shared: the table played
            // over by a script is placed with no view to take the layout's shape from.
            if side.is_none() != zones.shared {
                return;
            }
            if layout.pile_visible(zone) {
                lay_pile(placement, objects, (side, zone), (layout, camera));
            } else {
                lay_unseen_exile(placement, objects, side, (layout, camera));
            }
        }
        Group::Hand(side) => hand(
            placement,
            objects,
            (side, scroll.hand),
            (layout, &lift.hand),
            camera,
            hands,
        ),
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
