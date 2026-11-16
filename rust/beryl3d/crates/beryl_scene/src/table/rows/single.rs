//! Placement for rows that have no piles or tucked cards.

use beryl_protocol::protocol::ObjectView;

use super::{Laid, RowArea, RowRules, RowWindow, card_width, gaps, permanent};

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
    let gap = rules.layout.gap;
    let full = cards.iter().map(|card| card_width(card)).sum::<f32>() + gap * gaps(count);
    if full <= room || count == 0 {
        let mut left = if area.centred {
            area.left + (room - full) / 2.0
        } else {
            area.left
        };
        for (index, &card) in cards.iter().enumerate() {
            let width = card_width(card);
            laid(permanent(
                card,
                left + width / 2.0,
                y,
                u32::try_from(index).expect("rows hold at most u16::MAX cards"),
                true,
                1,
            ));
            left += width + gap;
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
    // Add the step at each edge as in the general layout so rounding stays the same.
    let last_left = (1..shown).fold(area.left, |left, _| left + step);
    let mut left = area.left;
    for (index, &card) in cards.iter().enumerate() {
        let (at, visible) = if index < first {
            (area.left, false)
        } else if index >= end {
            (last_left, false)
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
