//! Cards put back from the hand onto the library (`TrayKind::PutBack`): where they lie in the
//! tray, and where on the library a card let go lands.

use beryl_protocol::protocol::ObjectId;
use glam::Vec2;
use kit_base::geometry::Rect;
use kit_space::arrange::Grid;

use super::{Area, Held, Laid, Spot, TraySpace, curve_fan, height_of, rooms_of, widths};

/// Cards put back from the hand (the second pile) onto the library (the first), each pile laid
/// out for what it can come to hold, as the web's `putBack`: the library at the left of the
/// space, the cards put on it fanned down its face, the top one lowest and in front, and the
/// hand fanned at the right, or in a straight row in the space's `second` band where the layout
/// gives the hand one of its own: there the cards are no more than `least_step` apart, and a
/// card turned against its neighbour would cover the square it leaves. Side by side, the two piles' boxes share one strip and part half way between the
/// library and the fan, so that a card let go left of there goes on the library. Each card on
/// the library keeps at least `least_step` of its top uncovered, as each card of the fan keeps
/// that much of its left edge.
#[expect(clippy::cast_precision_loss, reason = "a tray holds a few dozen cards")]
pub(super) fn put_back(held: &Held, space: &TraySpace, (left, wide): (f32, f32)) -> Laid {
    let shape = &space.tuning.put_back;
    let gap = space.gap;
    let (onto, hand) = (&held.piles[0], &held.piles[1]);
    let rooms = rooms_of(held);
    let (deep, room) = (rooms[0] as f32, rooms[1] as f32);
    let own = space.second;
    let count = hand.len();
    // A hand in a band of its own is fanned across the whole column, as many as it holds now:
    // a hand that keeps room for every card the tray holds would close up below `least_step`.
    let fan_room = if own.is_some() { count as f32 } else { room };
    let (width, step) = sizes(space, wide, deep, fan_room);
    let height = height_of(space, width);
    let fanned = width + step * (room - 1.0).max(0.0);
    let whole = if own.is_some() {
        width
    } else {
        width * (1.0 + shape.between) + fanned
    };
    let library_x = left + gap.max((wide - whole) / 2.0);
    let span = f32::min(
        cascade(space, width),
        (space.height - 2.0 * gap - height) / (deep - 1.0).max(1.0),
    );
    let column = height + span * (deep - 1.0);
    let top = space.y + (space.height - column) / 2.0;
    let library = Area {
        x: library_x,
        y: top - shape.library_rise * height,
        width,
        height,
    };
    // The hand's fan, as long as the cards in it now, about the middle of the room it has. The
    // web draws the hand's cards first, then the library's column.
    let (hand_x, hand_wide, (hand_y, hand_height)) = match own {
        Some(own) => (left, wide, (own.y, own.height)),
        None => (
            library_x + width * (1.0 + shape.between),
            fanned,
            (space.y, space.height),
        ),
    };
    let start = hand_x + (hand_wide - (width + step * (count as f32 - 1.0))) / 2.0;
    // In a band of its own, the fan and its box are centred in the band; beside the library,
    // the fan's cards are centred in the space, as the web's are.
    let middle = if own.is_some() {
        hand_y + (hand_height - fan_height(space, width)) / 2.0 + space.heading + space.gap
    } else {
        hand_y + (hand_height - height) / 2.0
    };
    let (turn, drop) = if own.is_some() {
        (0.0, 0.0)
    } else {
        (shape.turn, shape.drop)
    };
    let mut laid = Laid::of(
        Vec::with_capacity(onto.len() + count),
        Vec::with_capacity(2),
    );
    let item = Vec2::new(width, height);
    let deck_grid = Grid::new(1, item, Vec2::new(0.0, span - height));
    let deck_region = Rect {
        x: library_x,
        y: top,
        width,
        height: column,
    };
    laid.grid(deck_grid, onto.len(), deck_region, false);
    laid.spots.reverse();
    for (index, spot) in laid.spots.iter_mut().enumerate() {
        let down = onto.len() - 1 - index;
        *spot = column_spot(
            (spot.x, spot.y, spot.width),
            shape.column_turns,
            index,
            down,
            count + down,
        );
    }
    let hand_grid = Grid::new(count, item, Vec2::new(step - width, 0.0));
    let hand_region = Rect {
        x: start,
        y: middle,
        width: hand_wide,
        height: height + drop * width,
    };
    laid.grid(hand_grid, count, hand_region, false);
    curve_fan(&mut laid.spots[onto.len()..], drop * width, turn);
    laid.boxes = drop_areas(
        (left, wide),
        (library.y - space.heading, top + column + gap),
        (
            middle - space.heading - gap,
            middle + height + drop * width + gap,
        ),
        own.is_none().then_some((library_x + width + hand_x) / 2.0),
    );
    laid.library = Some(library);
    laid.column = deck_grid
        .boxes(rooms[0], deck_region, false)
        .map(|rect| rect.y)
        .collect();
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

/// A card on a put-back's library at `x` and `y`, `width` wide, the `index`th of the library's
/// pile from the top and the `down`th from the bottom, drawn `drawn`th: the top card, in front,
/// lies straight, and those under it lean one way and the other.
fn column_spot(
    (x, y, width): (f32, f32, f32),
    turns: Vec2,
    index: usize,
    down: usize,
    drawn: usize,
) -> Spot {
    let [even, odd] = turns.to_array();
    Spot {
        x,
        y,
        width,
        turn: if index == 0 {
            0.0
        } else if down.is_multiple_of(2) {
            even
        } else {
            odd
        },
        drawn,
    }
}

/// How tall the box of a put-back's hand in a band of its own is, with cards `width` wide, from
/// its top edge, over the name, to its bottom edge, under the row of cards.
fn fan_height(space: &TraySpace, width: f32) -> f32 {
    space.heading + 2.0 * space.gap + height_of(space, width)
}

/// How far each card on a put-back's library, `width` wide, lies below the one under it: a
/// name bar's height, and at least the space's `least_step`.
fn cascade(space: &TraySpace, width: f32) -> f32 {
    f32::max(
        space.tuning.put_back.cascade * height_of(space, width),
        space.least_step,
    )
}

/// The card's width and the step between the hand's cards of a put-back laid out in `space`,
/// `wide` across, for a library that can come to hold `deep` cards and a fan of `room`: the widest
/// card for which the library's column fits the height and the hand's fan fits across, its
/// neighbours no closer than the space's `least_step` and, where the fan shares the library's
/// band, no closer than `least_step` of a card.
fn sizes(space: &TraySpace, wide: f32, deep: f32, room: f32) -> (f32, f32) {
    let shape = &space.tuning.put_back;
    let gap = space.gap;
    let tall = |width: f32| height_of(space, width) + cascade(space, width) * (deep - 1.0);
    let fan_step = |width: f32| {
        let across = if space.second.is_some() {
            wide
        } else {
            wide - 2.0 * gap - width * (1.0 + shape.between)
        };
        f32::min(
            f32::max(shape.step * width, space.least_step),
            (across - width) / (room - 1.0).max(1.0),
        )
    };
    let (mut width, mut step) = (space.least, 0.0);
    for tried in widths(space) {
        width = tried;
        step = fan_step(tried);
        let apart = (space.second.is_some() || step >= shape.least_step * tried)
            && (room <= 1.0 || step >= space.least_step);
        // A hand in a band of its own fits under its name, as the library's column does.
        let hand_fits = space
            .second
            .is_none_or(|own| fan_height(space, tried) <= own.height);
        if tall(tried) <= space.height - 2.0 * gap && apart && hand_fits {
            break;
        }
    }
    // A band too short for a hand of cards the least width takes cards as wide as it holds,
    // though narrower than the least.
    if let Some(own) = space.second
        && fan_height(space, width) > own.height
    {
        let most = (own.height - space.heading - 2.0 * gap) * space.card.width / space.card.height;
        width = most.max(space.least_step);
        step = fan_step(width);
    }
    (width, step)
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
