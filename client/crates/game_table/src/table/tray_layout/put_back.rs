//! Cards put back from the hand onto the library (`TrayKind::PutBack`): where they lie in the
//! tray, and where on the library a card let go lands.

use beryl_protocol::protocol::ObjectId;
use glam::Vec2;
use kit_base::geometry::Rect;
use kit_space::arrange::screen::{CascadeFan, FanShape, ScreenFit};

use super::{Held, Laid, TraySpace, rooms_of};

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
        cascade_rise: shape.pile_rise,
        base_edge: shape.backs_edge(),
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
    let mut laid = Laid::of(Vec::with_capacity(onto + hand), Vec::from(pair.bounds));
    laid.grid(pair.cascade);
    for spot in &mut laid.spots {
        spot.drawn += hand;
    }
    laid.spots.reverse();
    laid.grid(pair.fan);
    for (drawn, spot) in laid.spots[onto..].iter_mut().enumerate() {
        spot.drawn = drawn;
    }
    laid.library = Some(pair.base);
    laid.column = pair.slots.into_iter().map(|rect| rect.y).collect();
    laid
}

/// Where on the library of a put-back the card `carried`, let go with its middle at `point`,
/// lands, as the web's `library`: the place of the library's `column` (`cards` lie there now,
/// the top first) that, once the card is in it, lies nearest to where its middle was let go. A
/// held card is drawn with its middle under the pointer. Comparing that point with the cards'
/// current middles, as a row's landing does, would leave each place a strip only a name bar tall.
pub(super) fn library_at(
    cards: &[ObjectId],
    column: &[f32],
    library: Rect,
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
