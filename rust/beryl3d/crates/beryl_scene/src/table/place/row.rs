//! Battlefield row placement and the geometry shared by all placements in a window.

use beryl_protocol::protocol::{CardType, Combat, ObjectId, ObjectView, RowName, Side};

use std::borrow::Cow;

use super::{
    Group, PileRules, Placement, ROWS, Rest, Rules, Zones, host_slot, lay, members, row_index,
    side_index,
};
use crate::agent::hit;
use crate::frame::Place;
use crate::table::rows::{
    Beneath, Laid, NameIds, RowRules, SameName, lay_row, least_strip, row_length,
};
use crate::table::{CARD_HEIGHT, CARD_WIDTH, Camera, FittedRow, Layout, RowArea, row_area};
use glam::{Quat, Vec3};

/// The limits of a row for one layout and camera, independent of its cards and scroll offset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct RowFit {
    area: RowArea,
    y: f32,
    strip: f32,
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
                let strip = least_strip(camera, area, y, layout.strip + 2.0 * hit::CELL);
                RowFit { area, y, strip }
            })
        }))
    }

    pub(super) fn get(&self, side: Side, row: RowName) -> RowFit {
        self.0[side_index(side)][row_index(row)]
    }
}

/// How far each battlefield row's cards are shrunk from the size `web_fit` fits them at, by
/// side, the viewer's first, each in the order of `ROWS`. On the desktop layout fitted to a
/// window, a crowded row zooms the whole battlefield out, as the web interface's
/// `battlefieldZoom` (`ui/src/game/battlefield.ts`) does; on any other layout every row keeps
/// its full size.
///
/// It also keeps how far each row alone would need to shrink to fit its room, so that a step
/// that changes one row measures only that row again (`rezoom`). Two zooms are equal when their
/// rows are zoomed the same, whatever those fits.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Zooms {
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
    fn of_fits(fits: [[f32; 3]; 2]) -> Self {
        use crate::data::space::layout::desktop::LEAST_BACK_ZOOM;
        use crate::data::space::layout::desktop::web::{BACK_ROW_PULL, ZOOM_LEAST};
        let zoom = fits
            .iter()
            .fold(1.0_f32, |zoom, &[creatures, lands, others]| {
                zoom.min(creatures)
                    .min(lands.max(BACK_ROW_PULL))
                    .min(others.max(BACK_ROW_PULL))
            })
            .max(ZOOM_LEAST);
        let rows = fits.map(|[_, lands, others]| {
            [
                zoom,
                zoom.min(lands).max(LEAST_BACK_ZOOM),
                zoom.min(others).max(LEAST_BACK_ZOOM),
            ]
        });
        Self { rows, fits }
    }
}

impl Placement {
    /// A placement of nothing yet, whose rows are zoomed out by `zooms`. A script's steps keep
    /// the zooms the steps so far leave the table in the placement they lay groups out in, so
    /// that a step that changes no row's length need not find them again.
    pub(crate) fn zoomed(zooms: Zooms) -> Self {
        Self {
            zooms,
            ..Self::default()
        }
    }

    /// Forgets where `slot` rests.
    pub(crate) fn clear(&mut self, slot: ObjectId) {
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
    let (layout, _, _, fits, ids) = rules;
    if layout.fitted.is_none() {
        return Zooms::default();
    }
    let fit = |side: Side, row: RowName| {
        if !changed(side, row) {
            return known.fits[side_index(side)][row_index(row)];
        }
        let group = Group::Row { side, row };
        let cards = members(zones, group);
        let (cards, under, names) = prepared(&cards, row, ids);
        let (_, _, rules) = row_rules((layout, fits.get(side, row)), (side, row), 1.0, 0);
        let beneath = Beneath::new(under, &cards);
        let length = row_length(
            (&cards, &beneath),
            (names, ids, piles.for_group(group)),
            &rules,
        );
        if length > 0.0 {
            rules.room / length
        } else {
            1.0
        }
    };
    Zooms::of_fits([Side::You, Side::Opponent].map(|side| ROWS.map(|row| fit(side, row))))
}

/// The cards of a row as `lay_row` takes them: the row's permanents, lands in the order they
/// stand (`lands_in_order`) with each linked to the one before it with its name, and apart from
/// them the cards that lie under one of them. A row with no cards under its permanents, as
/// nearly every row is, keeps `cards` uncopied.
fn prepared<'a, 'b>(
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

/// How a row of `side` is laid out at `zoom` (`Zooms`) and scrolled `offset` groups: the row
/// fitted to the web interface's, if the layout fits its rows so; the scale its cards are drawn
/// at; and its rules, in millimetres of a card before that scale. A row so fitted is laid out in
/// an area as much wider than its own as its cards are smaller, and then scaled about its left
/// end (`web_fit`).
fn row_rules(
    (layout, fit): (&Layout, RowFit),
    (side, row): (Side, RowName),
    zoom: f32,
    offset: usize,
) -> (Option<FittedRow>, f32, RowRules<'_>) {
    let RowFit { area, strip, .. } = fit;
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
    let local = |x: f32| area.left + (x - area.left) / scale;
    let rules = RowRules {
        layout,
        strip: strip / scale,
        room: local(area.right) - area.left,
        scroll: offset,
        gap,
        tapped_gap,
        limit: local(limit),
        faces: &[],
    };
    (fitted, scale, rules)
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
    let (cards, under, names) = prepared(cards, row, ids);
    let (fitted, scale, rules) = row_rules((layout, fit), (side, row), zoom, offset);
    let local = |x: f32| area.left + (x - area.left) / scale;
    let unscaled = RowArea {
        y: area.y,
        left: area.left,
        right: local(area.right),
        centre: area.centre.map(local),
    };
    // A zoomed-out row keeps one edge where it stands at full size, as the web's does: a
    // creatures' row its edge toward the lane between the two creatures' rows, where combat and
    // the stack's arrows run, and a back row its edge toward its player. The far edge of the
    // table is up the screen, so the viewer's creatures and the opponent's back rows keep their
    // far edges, and move their line of centres that much further away.
    let line = fitted.map_or(y, |fitted| {
        let shrink = CARD_HEIGHT / 2.0 * fitted.scale * (1.0 - zoom) * fitted.tilt.cos();
        if (side == Side::You) == (row == RowName::Creatures) {
            y + shrink
        } else {
            y - shrink
        }
    });
    let height = fitted.map_or(0.0, |fitted| fitted.height(line));
    let on_line = (row, Vec3::new(0.0, line, height));
    let faces = faces(&cards, placement, (camera, area, on_line), local);
    let rules = RowRules {
        faces: &faces,
        ..rules
    };
    let place = |zone| Place::Card {
        zone,
        side,
        row: Some(row),
    };
    let mut set = |Laid {
                       object,
                       mut pose,
                       visible,
                       copies,
                   }| {
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
            // An attacker stands out of its row toward the other side, where the web's does.
            if matches!(object.combat, Some(Combat::Attacking { .. })) {
                pose.position.y += fitted.advance[usize::from(object.tapped)];
            }
            pose.position.z += fitted.height(pose.position.y);
            pose.rotation = Quat::from_rotation_x(fitted.tilt) * pose.rotation;
        }
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
    let beneath = Beneath::new(under, &cards);
    let window = lay_row(
        (&cards, &beneath),
        (names, ids, stack_slots),
        unscaled,
        y,
        &rules,
        &mut set,
    );
    placement.rows.battlefield[side_index(side)][row_index(row)] = window;
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
            faces.push((slot, x + (index as f32 - middle) * CARD_WIDTH * 1.04));
        }
    }
    faces
}

/// The battlefield rows of `zones` that hold a creature blocking an attacker. Each is laid out
/// again once every other row has its resting places, so that its blockers face where their
/// attackers rest (`faces`) whichever side's rows were laid out first.
pub(crate) fn blocking_rows<'z>(zones: &'z Zones<'_>) -> impl Iterator<Item = Group> + 'z {
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
pub(crate) fn blocked<'z>(zones: &'z Zones<'_>) -> impl Iterator<Item = ObjectId> + 'z {
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

/// Which row a permanent stands in: where a player pinned it, else the creatures' row for a
/// creature, the lands' part of the back row for a mana source, and the others' part for the
/// rest. A mana source is a land, or an artifact with a mana ability (a mana rock, a Treasure),
/// as the web interface's `battlefieldRow` has it.
pub(super) fn row_of(object: &ObjectView) -> RowName {
    let is = |kind| object.types.contains(&kind);
    object.pinned.unwrap_or(if is(CardType::Creature) {
        RowName::Creatures
    } else if is(CardType::Land) || (is(CardType::Artifact) && object.mana_ability) {
        RowName::Lands
    } else {
        RowName::Others
    })
}
