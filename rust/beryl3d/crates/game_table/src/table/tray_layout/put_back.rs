//! Cards put back from the hand onto the library (`TrayKind::PutBack`): where they lie in the
//! tray, and where on the library a card let go lands.

use beryl_protocol::protocol::ObjectId;
use glam::Vec2;
use kit_base::geometry::Rect;
use kit_space::arrange::screen::{CascadeFan, FanShape, ScreenFit};

use super::{Area, Held, Laid, TraySpace, rooms_of};

/// The library takes the cascade and the hand takes the fan. Drop areas include
/// the headings and the space reserved for cards that can move into the pile.
pub(super) fn put_back(held: &Held, space: &TraySpace, span: (f32, f32)) -> Laid {
    let shape = space.tuning.put_back;
    let [onto, hand] = [held.piles[0].len(), held.piles[1].len()];
    let rooms = rooms_of(held);
    let band = space.region(span);
    let second = space.second.map(|second| Rect {
        y: second.y,
        height: second.height.max(0.0),
        ..band
    });
    let policy = CascadeFan {
        cascade_step: shape.cascade,
        cascade_turns: shape.column_turns,
        separation: shape.between,
        fan: FanShape {
            step: shape.step,
            drop: if second.is_some() { 0.0 } else { shape.drop },
            turn: if second.is_some() { 0.0 } else { shape.turn },
        },
        fan_min_step: if second.is_some() {
            0.0
        } else {
            shape.least_step
        },
        preferred_step: space.least_step,
        heading: space.heading,
    };
    let pair = policy.fit(
        ScreenFit {
            press: space.press(),
            ..space.fit()
        },
        band,
        second,
        [onto, hand],
        [
            rooms[0].max(onto),
            if second.is_some() {
                hand
            } else {
                rooms[1].max(hand)
            },
        ],
    );
    let base = pair.slots[0];
    let library = Area {
        x: base.x,
        y: base.y - shape.pile_rise * base.height,
        width: base.width,
        height: base.height,
    };
    let deck = pair.cascade.viewport;
    let fan = pair.fan.viewport;
    let bottom = |rect: Rect| rect.y + rect.height;
    let deck_end = bottom(*pair.slots.last().expect("the library reserves a slot"));
    let deck_band = (
        library.y - space.heading,
        deck_end.min(bottom(deck)) + space.gap,
    );
    let fan_band = (fan.y - space.heading - space.gap, bottom(fan) + space.gap);
    let clamp_to_region = |(top, end): (f32, f32), region: Rect| {
        (
            top.clamp(region.y, bottom(region)),
            end.clamp(region.y, bottom(region)),
        )
    };
    let mut laid = Laid::of(Vec::with_capacity(onto + hand), Vec::with_capacity(2));
    laid.boxes = drop_areas(
        span,
        clamp_to_region(deck_band, band),
        clamp_to_region(fan_band, second.unwrap_or(band)),
        second
            .is_none()
            .then_some((base.x + base.width + fan.x) / 2.0),
    );
    laid.grid(pair.cascade);
    for spot in &mut laid.spots {
        spot.drawn += hand;
    }
    laid.spots.reverse();
    laid.grid(pair.fan);
    for (drawn, spot) in laid.spots[onto..].iter_mut().enumerate() {
        spot.drawn = drawn;
    }
    laid.library = Some(library);
    laid.column = pair.slots.into_iter().map(|rect| rect.y).collect();
    laid
}

/// The drop areas of the two piles. Stacked piles span their respective bands.
fn drop_areas(
    (left, wide): (f32, f32),
    deck: (f32, f32),
    fan: (f32, f32),
    split: Option<f32>,
) -> Vec<Area> {
    let strip = |(top, bottom): (f32, f32), x, width| Area {
        x,
        y: top,
        width,
        height: bottom - top,
    };
    if let Some(split) = split {
        let both = (deck.0.min(fan.0), deck.1.max(fan.1));
        vec![
            strip(both, left, split - left),
            strip(both, split, left + wide - split),
        ]
    } else {
        vec![strip(deck, left, wide), strip(fan, left, wide)]
    }
}

/// Where on the library of a put-back the card `carried`, let go with its middle at `point`,
/// lands, as the web's `library`: the place of the library's `column` (`cards` lie there now,
/// the top first) that, once the card is in it, lies nearest to where its middle was let go. A
/// held card is drawn with its middle under the pointer (`pointer::held::pose`), so the point is
/// its middle. Comparing the point with the middles of the cards, as a row's landing does, would
/// leave each place a strip only a name bar tall.
pub(super) fn library_at(
    cards: &[ObjectId],
    column: &[f32],
    library: Area,
    point: Vec2,
    carried: ObjectId,
) -> usize {
    let own = cards.iter().position(|&card| card == carried);
    let others = cards.len() - usize::from(own.is_some());
    // The places it could take, the deepest first: one more than the others hold.
    let places = &column[..column.len().min(others + 1)];
    let off = |top: f32| (top + library.height / 2.0 - point.y).abs();
    let nearest = (0..places.len())
        .min_by(|&a, &b| off(places[a]).total_cmp(&off(places[b])))
        .unwrap_or(0);
    // Its index among the others, the top first; below where it lies now, one more, as the
    // drop's `at` counts the card itself in its place (the session takes it out before putting
    // it back).
    let among = others - nearest;
    match own {
        Some(own) if among > own => among + 1,
        _ => among,
    }
}
