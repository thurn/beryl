//! Battlefield row placement and the geometry shared by all placements in a window.

use beryl_protocol::protocol::{ObjectId, ObjectView, RowName, Side};

use super::{Placement, ROWS, Rest, host_slot, row_index, side_index};
use crate::agent::hit;
use crate::frame::Place;
use crate::table::rows::{Beneath, Laid, NameIds, RowRules, SameName, lay_row, least_strip};
use crate::table::{Camera, Layout, RowArea, row_area};

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
/// order, except that lands stand untapped first, so how much mana is open reads at a glance,
/// and each name together, in the order the names first appear.
pub(super) fn battlefield_row(
    placement: &mut Placement,
    cards: &[&ObjectView],
    (side, row): (Side, RowName),
    (layout, fit): (&Layout, RowFit),
    offset: usize,
    ids: Option<&NameIds<'_>>,
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
    let rules = RowRules {
        layout,
        strip,
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
    if !names.repeats() {
        let mut order = Vec::with_capacity(cards.len());
        for tapped in [false, true] {
            order.extend(cards.iter().copied().filter(|card| card.tapped == tapped));
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
