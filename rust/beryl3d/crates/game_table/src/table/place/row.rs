//! Battlefield row placement and the geometry shared by all placements in a window.

use beryl_protocol::protocol::{CardType, Combat, ObjectId, ObjectView, RowName, Side};

use std::borrow::Cow;

use super::{
    Group, PileRules, Placement, ROWS, Rest, Rules, Zones, host_slot, lay, members, row_index,
    side_index,
};
use crate::table::rows::{Beneath, NameIds, SameName, face_up, grouped};
use crate::table::{Camera, FittedRow, Layout, RowArea, RowWindow, RowZoom, row_area};
use glam::{Quat, Vec3};
use kit_frame::frame::Place;
use kit_scene::hit::CELL;
use kit_space::arrange::row::{self as solver, Row};

/// The limits of a row for one layout and camera, independent of its cards and scroll offset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct RowFit {
    area: RowArea,
    y: f32,
    row: Row,
}

/// The six rows' geometry. The table rebuilds it when it fits its camera to a new window.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RowFits([[RowFit; 3]; 2]);

impl RowFits {
    pub(crate) fn new(layout: &Layout, camera: &Camera) -> Self {
        Self([Side::You, Side::Opponent].map(|side| {
            ROWS.map(|row| {
                let (area, y) = row_area((layout, camera), side, row);
                // The reachable check can miss one grid cell at either edge of the strip.
                let row = Row::new(
                    [area.left, area.right],
                    area.centre,
                    y,
                    camera,
                    (camera.data.card.width, layout.strip + 2.0 * CELL),
                );
                RowFit { area, y, row }
            })
        }))
    }

    pub(super) fn get(&self, side: Side, row: RowName) -> RowFit {
        self.0[side_index(side)][row_index(row)]
    }
}

/// How far each battlefield row's cards are shrunk from their fitted size, by
/// side, the viewer's first, each in the order of `ROWS`. On the desktop layout fitted to a
/// window, a crowded row zooms the whole battlefield out, as the web interface's
/// `battlefieldZoom` (`ui/src/game/battlefield.ts`) does; on any other layout every row keeps
/// its full size.
///
/// It also keeps how far each row alone would need to shrink to fit its room, so that a step
/// that changes one row measures only that row again (`rezoom`). Two zooms are equal when their
/// rows are zoomed the same, whatever those fits.
#[derive(Clone, Copy, Debug)]
pub struct Zooms {
    rows: [[f32; 3]; 2],
    fits: [[f32; 3]; 2],
}

impl Default for Zooms {
    fn default() -> Self {
        Self {
            rows: [[1.0; 3]; 2],
            fits: [[1.0; 3]; 2],
        }
    }
}

impl PartialEq for Zooms {
    fn eq(&self, other: &Self) -> bool {
        self.rows == other.rows
    }
}

impl Zooms {
    /// How far the row `row` of `side` is zoomed out.
    pub(crate) fn get(&self, side: Side, row: RowName) -> f32 {
        self.rows[side_index(side)][row_index(row)]
    }

    /// The zoom of every row, from how far each row alone would need to shrink to fit its room.
    /// The battlefield zooms out as far as its most crowded row, on either side, needs to fit
    /// its room, down to `ZOOM_LEAST`; a back row (lands or others) pulls it no lower than
    /// `BACK_ROW_PULL`. The creatures' rows take that zoom. A back row takes it too, or shrinks
    /// further alone to fit its room. The web lets its lands shrink without end and its others'
    /// row down to `ZOOM_LEAST`; here either back row stops at `LEAST_BACK_ZOOM` and past it
    /// crowds and scrolls, since a land at a zoom of 0.3 is about 28 points wide, too narrow to
    /// leave the reachable check's 24-point square inside its slanted edges.
    fn of_fits(fits: [[f32; 3]; 2], limits: RowZoom) -> Self {
        let back = Some(limits.back);
        let rows = solver::linked_scales(fits, limits.minimum, [None, back, back]);
        Self { rows, fits }
    }
}

impl Placement {
    /// A placement of nothing yet, whose rows are zoomed out by `zooms`. A script's steps keep
    /// the zooms the steps so far leave the table in the placement they lay groups out in, so
    /// that a step that changes no row's length need not find them again.
    pub fn zoomed(zooms: Zooms) -> Self {
        Self {
            zooms,
            ..Self::default()
        }
    }

    /// Forgets where `slot` rests.
    pub fn clear(&mut self, slot: ObjectId) {
        if let Some(rest) = self.rests.get_mut(usize::from(slot.0)) {
            *rest = None;
        }
    }
}

/// The zoom of every row of `zones` (`Zooms::of_fits`).
pub(crate) fn zooms(zones: &Zones, piles: &PileRules, rules: Rules) -> Zooms {
    rezoom(&Zooms::default(), zones, piles, rules, |_, _| true)
}

/// The zoom of every row of `zones`, measuring again only the rows `changed` names and taking
/// the others' fits from `known`, the zooms of the same rows before. Measuring every row on
/// each step of a crowded board cost more than laying out the rows that step changed.
pub(crate) fn rezoom(
    known: &Zooms,
    zones: &Zones,
    piles: &PileRules,
    rules: Rules,
    changed: impl Fn(Side, RowName) -> bool,
) -> Zooms {
    let (layout, _, _, fits, ids, _) = rules;
    let Some(limits) = layout.row_zoom else {
        return Zooms::default();
    };
    let fit = |side: Side, row: RowName| {
        if !changed(side, row) {
            return known.fits[side_index(side)][row_index(row)];
        }
        let group = Group::Row { side, row };
        let cards = members(zones, group);
        let (cards, under, names) = prepared(&cards, row, ids);
        let (_, _, container) = row_rules((layout, fits.get(side, row)), (side, row), 1.0);
        let beneath = Beneath::new(under, &cards);
        let groups = grouped((&cards, &beneath), (names, ids, piles.for_group(group)));
        container.fit(groups.solver_groups(&beneath, layout, &[]))
    };
    Zooms::of_fits(
        [Side::You, Side::Opponent].map(|side| ROWS.map(|row| fit(side, row))),
        limits,
    )
}

/// The cards of a row as the row solver takes them: the row's permanents, lands in the order they
/// stand (`lands_in_order`) with each linked to the one before it with its name, and apart from
/// them the cards that lie under one of them. A row with no cards under its permanents, as
/// nearly every row is, keeps `cards` uncopied.
pub fn prepared<'a, 'b>(
    cards: &'b [&'a ObjectView],
    row: RowName,
    ids: Option<&NameIds<'_>>,
) -> (
    Cow<'b, [&'a ObjectView]>,
    Vec<&'a ObjectView>,
    Option<SameName>,
) {
    let in_row = |slot: ObjectId| cards.iter().copied().find(|object| object.slot == slot);
    let lies_under = |object: &&ObjectView| host_slot(object, in_row).is_some();
    let (permanents, under) = if cards.iter().any(lies_under) {
        let (under, permanents): (Vec<_>, Vec<_>) = cards.iter().copied().partition(lies_under);
        (Cow::Owned(permanents), under)
    } else {
        (Cow::Borrowed(cards), Vec::new())
    };
    if row == RowName::Lands {
        let (sorted, names) = lands_in_order(&permanents, ids);
        return (Cow::Owned(sorted), under, Some(names));
    }
    (permanents, under, None)
}

/// The projected row, its scale, and the container measured in units of an unscaled card.
/// The cards are scaled about the row's left end.
fn row_rules(
    (layout, fit): (&Layout, RowFit),
    (side, row): (Side, RowName),
    zoom: f32,
) -> (Option<FittedRow>, f32, Row) {
    let RowFit {
        area,
        row: container,
        ..
    } = fit;
    // The rightmost any row of the layout reaches, past which no card scrolled off this row's
    // right end rests.
    let rights = match &layout.fitted {
        Some(rows) => rows[side_index(side)].map(|row| row.area.right),
        None => [layout.creatures, layout.lands, layout.others].map(|row| row.right),
    };
    let limit = rights.into_iter().fold(area.right, f32::max);
    let fitted = layout
        .fitted
        .map(|rows| rows[side_index(side)][row_index(row)]);
    let (scale, gap, tapped_gap) = fitted.map_or((1.0, layout.gap, layout.gap), |fitted| {
        (fitted.scale * zoom, fitted.gap, fitted.tapped_gap)
    });
    let container = container
        .with_spacing(
            (gap, tapped_gap),
            layout.pile_step,
            layout.beneath_step.x * layout.camera.card.width,
            limit,
        )
        .at_scale(scale);
    (fitted, scale, container)
}

/// The fit chosen for a battlefield container during an inspection solve.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RowDecision {
    pub items: usize,
    pub groups: usize,
    pub zoom: f32,
    pub least_zoom: Option<f32>,
    pub fit: solver::Decision,
    pub region: &'static str,
}

/// One of one side's three battlefield rows, `cards` in the order they entered. They keep that
/// order, except that mana sources stand untapped first, so how much mana is open reads at a
/// glance, lands before artifacts, and each name together, in the order the names first appear.
pub(super) fn battlefield_row(
    placement: &mut Placement,
    cards: &[&ObjectView],
    (side, row): (Side, RowName),
    (layout, fit, camera): (&Layout, RowFit, &Camera),
    offset: usize,
    (ids, stack_slots): (Option<&NameIds<'_>>, Option<&[ObjectId]>),
) {
    let RowFit { area, y, .. } = fit;
    let zoom = placement.zooms.get(side, row);
    let items = cards.len();
    let (cards, under, names) = prepared(cards, row, ids);
    let (fitted, scale, container) = row_rules((layout, fit), (side, row), zoom);
    // A zoomed-out row keeps one edge where it stands at full size, as the web's does: a
    // creatures' row its edge toward the lane between the two creatures' rows, where combat and
    // the stack's arrows run, and a back row its edge toward its player. The far edge of the
    // table is up the screen, so the viewer's creatures and the opponent's back rows keep their
    // far edges, and move their line of centres that much further away.
    let line = fitted.map_or(y, |fitted| {
        let shrink =
            camera.data.card.height / 2.0 * fitted.scale * (1.0 - zoom) * fitted.tilt.cos();
        if (side == Side::You) == (row == RowName::Creatures) {
            y + shrink
        } else {
            y - shrink
        }
    });
    let height = fitted.map_or(0.0, |fitted| fitted.height(line, camera.data.card));
    let on_line = (row, Vec3::new(0.0, line, height));
    let faces = faces(&cards, placement, (camera, area, on_line), |x| {
        fit.row.unscaled(x, scale)
    });
    let beneath = Beneath::new(under, &cards);
    let groups = grouped((&cards, &beneath), (names, ids, stack_slots));
    let inputs: Vec<_> = groups.solver_groups(&beneath, layout, &faces).collect();
    let place = |zone| Place::Card {
        zone,
        side,
        row: Some(row),
    };
    let posed = |placed: &solver::Placed| {
        let (object, mut pose) = groups.pose(&beneath, placed, (y, camera.data.card));
        if let Some(fitted) = fitted {
            let at = pose.position;
            pose.position = Vec3::new(
                area.left + (at.x - area.left) * scale,
                line + (at.y - y) * scale,
                at.z * scale,
            );
            pose.scale *= scale;
            // Each card leans back toward the camera in the row's plane, as far as makes it as
            // tall on screen as the web's upright card. A tapped card keeps its centre on the
            // row's line, where the web lines a row's cards up by their tops: one raised up the
            // table would lie farther from the camera than the untapped card beside it, and the
            // hit test, which takes the nearer card for the one in front, would let that card
            // cover it.
            // An attacker stands out of its row toward the other side, where the web's does. It
            // keeps the height of its place in the row: the row's plane rises from the near edge
            // of an untapped card, so the opponent's attacker, moved toward the viewer, would
            // otherwise sink with its near edge and its wedge under the table.
            pose.position.z += fitted.height(pose.position.y, camera.data.card);
            if matches!(object.combat, Some(Combat::Attacking { .. })) {
                pose.position.y += fitted.advance[usize::from(object.tapped)];
            }
            pose.rotation = Quat::from_rotation_x(fitted.tilt) * pose.rotation;
            // A drawn card is sheared until its box on screen is the web's skewed card's. A card
            // scrolled out of view rests undrawn at the row's end, under the cards that join
            // it, so it keeps its pose: sheared, its pose would change with every card that
            // joins and give it a track for nothing.
            if placed.visible {
                pose = layout
                    .region_fit
                    .row_pose(&fitted, zoom, camera, pose, object.tapped);
            }
        }
        (object, pose)
    };
    let project = |placed: &solver::Placed| {
        let (object, pose) = posed(placed);
        let pressable = placed.copies > 0 && face_up(object) && object.name.as_deref() != Some("");
        (camera.card_corners(pose.transform()), pressable)
    };
    let capture = placement.decisions.is_some();
    let restore = |placed: solver::Placed| {
        let (object, pose) = posed(&placed);
        placement.set(
            object.slot,
            Rest {
                pose,
                place: place(object.zone),
                visible: placed.visible,
                copies: u16::try_from(placed.copies).unwrap_or(u16::MAX),
            },
        );
    };
    let mut decision = solver::Decision::default();
    let window = solver::arrange_inspected(
        container,
        &inputs,
        offset,
        (camera, layout.strip),
        project,
        restore,
        capture.then_some(&mut decision),
    );
    if let Some(rows) = placement.decisions.as_mut() {
        rows.battlefield[side_index(side)][row_index(row)] = RowDecision {
            items,
            groups: inputs.len(),
            zoom,
            least_zoom: layout.row_zoom.map(|limits| {
                if row == RowName::Creatures {
                    limits.minimum
                } else {
                    limits.back.minimum
                }
            }),
            fit: decision,
            region: region_key(fitted.is_some(), side, row),
        };
    }
    placement.rows.battlefield[side_index(side)][row_index(row)] = RowWindow::new(
        window.scroll.first,
        window.scroll.shown,
        window.scroll.last,
        (window.before, window.after),
    );
}

fn region_key(fitted: bool, side: Side, row: RowName) -> &'static str {
    match (fitted, side, row) {
        (false, _, RowName::Creatures) => "table.creatures",
        (false, _, RowName::Lands) => "table.lands",
        (false, _, RowName::Others) => "table.others",
        (true, _, RowName::Creatures) => "table.anchored.web.room",
        (true, Side::You, RowName::Lands | RowName::Others) => "table.anchored.web.you",
        (true, Side::Opponent, RowName::Lands | RowName::Others) => "table.anchored.web.opponent",
    }
}

/// Where each blocker of `cards` wants its middle to stand so that it faces its attacker
/// straight across the table, as the web interface's `row` (`ui/src/game/battlefield.ts`)
/// stands it, in the row's millimetres before it is scaled (`local` turns a table x into
/// them): under its attacker's middle on the screen, and an attacker's blockers side by side
/// about that, 1.04 card widths apart, in the creatures' row only. `line` is the camera, the
/// row's area, the row and a point on its line at the height its cards rest at. A blocker
/// whose attacker has no resting place yet faces nothing; `place_zones` lays such a row out
/// again once every row has been placed.
fn faces(
    cards: &[&ObjectView],
    placement: &Placement,
    (camera, area, (row, line)): (&Camera, RowArea, (RowName, Vec3)),
    local: impl Fn(f32) -> f32,
) -> Vec<(ObjectId, f32)> {
    let attacker = |object: &ObjectView| match object.combat {
        Some(Combat::Blocking { attacker }) => attacker,
        _ => None,
    };
    if row != RowName::Creatures || !cards.iter().any(|card| attacker(card).is_some()) {
        return Vec::new();
    }
    // The row's line is level with the camera's sideways axis, so a point's x on the screen
    // moves with its x on the table in proportion along it.
    let at = |x: f32| camera.to_screen(Vec3::new(x, line.y, line.z)).x;
    let (left, right) = (at(area.left), at(area.right));
    let across = right - left;
    let mut faces = Vec::new();
    for card in cards {
        let Some(blocked) = attacker(card) else {
            continue;
        };
        let Some(rest) = placement.get(blocked) else {
            continue;
        };
        if across.abs() < f32::EPSILON || faces.iter().any(|(slot, _)| *slot == card.slot) {
            continue;
        }
        let screen = camera.to_screen(rest.pose.position).x;
        let x = local(area.left + (screen - left) / across * (area.right - area.left));
        let together: Vec<ObjectId> = cards
            .iter()
            .filter(|other| attacker(other) == Some(blocked))
            .map(|other| other.slot)
            .collect();
        let middle = (together.len() - 1) as f32 / 2.0;
        for (index, slot) in together.into_iter().enumerate() {
            faces.push((
                slot,
                x + (index as f32 - middle) * camera.data.card.width * 1.04,
            ));
        }
    }
    faces
}

/// The battlefield rows of `zones` that hold a creature blocking an attacker. Each is laid out
/// again once every other row has its resting places, so that its blockers face where their
/// attackers rest (`faces`) whichever side's rows were laid out first.
pub fn blocking_rows<'z>(zones: &'z Zones<'_>) -> impl Iterator<Item = Group> + 'z {
    [Side::You, Side::Opponent]
        .into_iter()
        .map(|side| Group::Row {
            side,
            row: RowName::Creatures,
        })
        .filter(|&group| blockers(zones).any(|(blocker, _)| zones.group_of(blocker) == Some(group)))
}

/// Lays out again each row of `zones` that holds a blocker (`blocking_rows`), once every row
/// has been placed, and returns how many objects it laid out.
pub(super) fn lay_blockers_again(
    (zones, piles, rules): (&Zones<'_>, &PileRules, Rules<'_>),
    placement: &mut Placement,
) -> usize {
    let mut laid = 0;
    for group in blocking_rows(zones) {
        let objects = members(zones, group);
        laid += objects.len();
        lay(group, &objects, (zones, piles, rules, None), placement);
    }
    laid
}

/// The attackers that creatures of `zones` block.
pub fn blocked<'z>(zones: &'z Zones<'_>) -> impl Iterator<Item = ObjectId> + 'z {
    blockers(zones).map(|(_, attacker)| attacker)
}

/// Each permanent of `zones` that blocks an attacker, with that attacker. It reads the
/// battlefield's list of slots, so that it takes no list of its own to say there is none.
fn blockers<'z>(zones: &'z Zones<'_>) -> impl Iterator<Item = (&'z ObjectView, ObjectId)> + 'z {
    zones.battlefield.iter().filter_map(|&slot| {
        let object = zones.get(slot)?;
        match object.combat {
            Some(Combat::Blocking {
                attacker: Some(attacker),
            }) => Some((object, attacker)),
            _ => None,
        }
    })
}

/// A lands row's cards in the order they stand: untapped first, then lands before the mana
/// artifacts, as the web interface's `sideGroups` has it, and each name together, in the order
/// the names first appear in `cards`. Otherwise the order of `cards` is kept. Also links each
/// card, in that order, to the one before it with its name.
fn lands_in_order<'a>(
    cards: &[&'a ObjectView],
    ids: Option<&NameIds<'_>>,
) -> (Vec<&'a ObjectView>, SameName) {
    let names = SameName::of(cards, ids);
    let land = |object: &ObjectView| object.types.contains(&CardType::Land);
    let groups = [(false, true), (false, false), (true, true), (true, false)];
    if !names.repeats() {
        let mut order = Vec::with_capacity(cards.len());
        for (tapped, lands) in groups {
            order.extend(
                (cards.iter().copied()).filter(|card| card.tapped == tapped && land(card) == lands),
            );
        }
        return (order, names);
    }
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
    // Each name's cards, untapped and then tapped, lands first, from its first card. `latest`
    // holds where the card last put in order with each name went, by the index of that name's
    // first card.
    let mut order = Vec::with_capacity(cards.len());
    let mut previous = Vec::with_capacity(cards.len());
    let mut latest: Vec<Option<usize>> = vec![None; cards.len()];
    for (tapped, lands) in groups {
        for (index, &first) in first.iter().enumerate() {
            if first != index {
                continue;
            }
            let mut next = Some(index);
            while let Some(card) = next {
                if cards[card].tapped == tapped && land(cards[card]) == lands {
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

/// The battlefield row for a permanent. A pin takes precedence over card types and mana
/// abilities, as in the web interface's `battlefieldRow`. Artifacts with mana abilities
/// go in the lands row.
#[must_use]
pub fn row_of(object: &ObjectView) -> RowName {
    let is = |kind| object.types.contains(&kind);
    object.pinned.unwrap_or(if is(CardType::Creature) {
        RowName::Creatures
    } else if is(CardType::Land) || (is(CardType::Artifact) && object.mana_ability) {
        RowName::Lands
    } else {
        RowName::Others
    })
}
