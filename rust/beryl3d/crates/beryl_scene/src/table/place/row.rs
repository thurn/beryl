//! Battlefield row placement and the geometry shared by all placements in a window.

use beryl_protocol::protocol::{CardType, ObjectId, ObjectView, RowName, Side};

use super::{Placement, ROWS, Rest, host_slot, row_index, side_index};
use crate::agent::hit;
use crate::frame::Place;
use crate::table::rows::{Beneath, Laid, NameIds, RowRules, SameName, lay_row, least_strip};
use crate::table::{Camera, Layout, RowArea, row_area};
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

/// One of one side's three battlefield rows, `cards` in the order they entered. They keep that
/// order, except that mana sources stand untapped first, so how much mana is open reads at a
/// glance, lands before artifacts, and each name together, in the order the names first appear.
pub(super) fn battlefield_row(
    placement: &mut Placement,
    cards: &[&ObjectView],
    (side, row): (Side, RowName),
    (layout, fit): (&Layout, RowFit),
    offset: usize,
    (ids, stack_slots): (Option<&NameIds<'_>>, Option<&[ObjectId]>),
) {
    let RowFit { area, y, strip } = fit;
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
    // The rightmost any row of the layout reaches, past which no card scrolled off this row's
    // right end rests.
    let rights = match &layout.fitted {
        Some(rows) => rows[side_index(side)].map(|row| row.area.right),
        None => [layout.creatures, layout.lands, layout.others].map(|row| row.right),
    };
    let limit = rights.into_iter().fold(area.right, f32::max);
    // A row fitted to the web interface's is laid out in millimetres of a card at its printed
    // size, in an area as much wider than its own as its cards are smaller, and then scaled
    // about its left end (`web_fit`).
    let fitted = layout
        .fitted
        .map(|rows| rows[side_index(side)][row_index(row)]);
    let (scale, gap, tapped_gap) = fitted.map_or((1.0, layout.gap, layout.gap), |fitted| {
        (fitted.scale, fitted.gap, fitted.tapped_gap)
    });
    let local = |x: f32| area.left + (x - area.left) / scale;
    let unscaled = RowArea {
        y: area.y,
        left: area.left,
        right: local(area.right),
        centre: area.centre.map(local),
    };
    let rules = RowRules {
        layout,
        strip: strip / scale,
        room: unscaled.right - unscaled.left,
        scroll: offset,
        gap,
        tapped_gap,
        limit: local(limit),
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
                y + (at.y - y) * scale,
                at.z * scale,
            );
            pose.scale *= scale;
            // Each card leans back toward the camera in the row's plane, as far as makes it as
            // tall on screen as the web's upright card. A tapped card keeps its centre on the
            // row's line, where the web lines a row's cards up by their tops: one raised up the
            // table would lie farther from the camera than the untapped card beside it, and the
            // hit test, which takes the nearer card for the one in front, would let that card
            // cover it.
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
    let beneath = Beneath::new(under, cards);
    placement.rows.battlefield[side_index(side)][row_index(row)] = lay_row(
        (cards, &beneath),
        (names, ids, stack_slots),
        unscaled,
        y,
        &rules,
        &mut set,
    );
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
