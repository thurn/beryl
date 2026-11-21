//! Placement for rows that have no piles or tucked cards.

use beryl_protocol::protocol::ObjectView;

use super::{Laid, RowArea, RowRules, RowWindow, card_width, faced, gaps, permanent};

/// Lays a row with no piles or tucked cards. Each card's width and height in the draw order
/// can be read directly, so the row needs no list of group widths or group slices.
pub(super) fn lay_single_cards<'a>(
    cards: &[&'a ObjectView],
    area: RowArea,
    y: f32,
    rules: &RowRules,
    laid: &mut impl FnMut(Laid<'a>),
) -> RowWindow {
    let room = area.right - area.left;
    let count = cards.len();
    let between = |index: usize| {
        if index == 0 {
            0.0
        } else {
            rules.gap_between(cards[index - 1], cards[index])
        }
    };
    let full = cards.iter().map(|card| card_width(card)).sum::<f32>()
        + (0..count).map(between).sum::<f32>();
    if full <= room || count == 0 {
        let widths: Vec<f32> = if rules.faces.is_empty() {
            Vec::new()
        } else {
            cards.iter().map(|card| card_width(card)).collect()
        };
        if let Some((order, lefts)) = faced(cards, &widths, (area, full), rules) {
            for (height, (&index, &left)) in order.iter().zip(&lefts).enumerate() {
                let card = cards[index];
                laid(permanent(
                    card,
                    left + card_width(card) / 2.0,
                    y,
                    u32::try_from(height).expect("rows hold at most u16::MAX cards"),
                    true,
                    1,
                ));
            }
            return RowWindow::new(count, 0, count, (0, 0));
        }
        let mut left = RowRules::start(area, full);
        for (index, &card) in cards.iter().enumerate() {
            left += between(index);
            let width = card_width(card);
            laid(permanent(
                card,
                left + width / 2.0,
                y,
                u32::try_from(index).expect("rows hold at most u16::MAX cards"),
                true,
                1,
            ));
            left += width;
        }
        return RowWindow::new(count, 0, count, (0, 0));
    }
    let widest = cards
        .iter()
        .map(|card| card_width(card))
        .fold(0.0, f32::max);
    let shown = window_size(count, room, widest, rules.strip);
    let first = rules.scroll.min(count - shown);
    let end = first + shown;
    let step = spacing(shown, room, card_width(cards[end - 1]), 0.0);
    let mut left = area.left;
    for (index, &card) in cards.iter().enumerate() {
        let (at, visible) = if index < first {
            (area.left, false)
        } else if index >= end {
            (beyond_right(area, rules, card_width(card)), false)
        } else {
            let at = left;
            left += step;
            (at, true)
        };
        laid(permanent(
            card,
            at + card_width(card) / 2.0,
            y,
            u32::try_from(index).expect("rows hold at most u16::MAX cards"),
            visible,
            1,
        ));
    }
    RowWindow::new(count, first, shown, (first, count - end))
}

/// The left edge at which a card or group `width` wide that is scrolled off a row's right end
/// rests, undrawn: one `gap` past that end, beside the count of the cards off it, but no farther
/// right than the right end of the layout's rightmost row, inside which the camera keeps the
/// window. A card that flies to the row's hidden end is drawn while it flies. Resting at the last
/// card in view, which stands against the row's right end, it would end its flight under that
/// card and the cards flying after it, and a card flying from that place would hardly move. On
/// the phone layout, where a crowded lands row shows two cards and ends in the middle of the
/// screen, that hid most flights into the row. A row that ends at the battlefield's right edge
/// has no room past its end, and its hidden cards rest against that end.
pub(super) fn beyond_right(area: RowArea, rules: &RowRules, width: f32) -> f32 {
    (area.right + rules.gap).min(rules.limit.max(area.right) - width)
}

pub(super) fn window_size(groups: usize, room: f32, widest: f32, strip: f32) -> usize {
    ((((room - widest) / strip).floor() as usize) + 1).clamp(1, groups)
}

pub(super) fn spacing(shown: usize, room: f32, last_width: f32, extra: f32) -> f32 {
    if shown > 1 {
        ((room - last_width - extra) / gaps(shown)).max(0.0)
    } else {
        0.0
    }
}
