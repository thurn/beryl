//! The pill that says what a prompt asks on the phone, and where it stands.

use beryl_protocol::prompt::PromptView;
use glam::Vec2;
use kit_base::geometry::Rect;
use kit_base::tokens::{color, type_style};
use kit_scene::Candidate;
use kit_text::text::TypeStyle;
use kit_ui::element::{Align, Anchor, Element, TextAlign, column, text};

use super::{PromptSlots, table_rows};
use crate::hud::placement_scene::{Group, PlacementScene, inset, rect};

/// Where the pill with what a prompt asks stands on the phone.
#[derive(Clone, Copy)]
pub(super) enum PillPlace {
    /// Centred on the middle band's line, or at the nearest height clear of every card.
    Middle,
    /// At the same height, narrowed for buttons this wide at the middle band's left or right
    /// end.
    Beside { left: f32, right: f32 },
    /// Over the viewer's strip, its foot at this height, beside the stacked buttons, while the
    /// table lies under a shade.
    Over(f32),
}

impl PillPlace {
    pub(super) fn beside_controls(held: bool, over: f32, left: f32, right: f32) -> Self {
        if held {
            Self::Over(over)
        } else if (left > 0.0) == (right > 0.0) {
            // Buttons at both ends can leave too little room to wrap the pill without covering a row.
            Self::Middle
        } else {
            Self::Beside { left, right }
        }
    }
}

/// The pill that says what `prompt` asks, `asks`, at `place`, with the prompt's subtitle on a
/// line under it where it has one: a fixed width, so that a long question is laid out on as
/// many lines as it takes. It lets the pointer through, so it never covers a card the prompt
/// picks.
pub(super) fn pill(
    asks: &str,
    prompt: &PromptView,
    place: PillPlace,
    slots: &PromptSlots<'_>,
) -> (Element, Rect) {
    let style = TypeStyle {
        size: slots.metrics.pill_text,
        ..type_style::TITLE
    };
    let padding = slots.metrics.pill_padding;
    let line = style.size * style.line_height + padding;
    // The subtitle's body type is scaled down as far as the pill's title is from the dock's, but
    // never under the least text size the phone allows.
    let below_size =
        (type_style::BODY.size * style.size / type_style::TITLE.size).max(slots.text_points);
    let below = super::subtitle(prompt, below_size);
    let height = line
        + below
            .as_ref()
            .map_or(0.0, |_| below_size * type_style::BODY.line_height);
    let room = |taken: f32| {
        if taken > 0.0 {
            taken + slots.metrics.gap
        } else {
            0.0
        }
    };
    let (left, right) = match place {
        PillPlace::Middle => (0.0, 0.0),
        PillPlace::Beside { left, right } => (room(left), room(right)),
        PillPlace::Over(_) => (0.0, room(slots.metrics.button_min_width)),
    };
    let area = if matches!(place, PillPlace::Over(_)) {
        slots.asked
    } else {
        slots.bar
    };
    let (x, end) = if matches!(place, PillPlace::Over(_)) {
        (area.x + left, area.x + area.width - right)
    } else {
        // On the middle band's line, the pill stands beside the phase icon and between the
        // shared piles, if the game has them. Its rounded end may run under the icon by its
        // padding, which holds no text, so that a crowded band leaves its words more room.
        let padding = slots.metrics.pill_padding;
        let (phase_left, phase_right) = slots.phase;
        let (start, end) = slots.middle_room();
        let band_end = slots.middle.x + slots.middle.width;
        let (start, end) = (
            start.min(phase_right - padding).max(slots.middle.x),
            end.max(phase_left + padding).min(band_end),
        );
        let beside = (
            (area.x + left).max(start),
            (area.x + area.width - right).min(end),
        );
        // Where that is too narrow for the longest word, the pill takes the icon's room, and
        // the bar leaves the icon out (`PromptSlots::phase_icon`).
        if beside.1 - beside.0 - 2.0 * padding >= slots.word_width(asks) {
            beside
        } else {
            (
                (area.x + left).max(slots.middle.x),
                (area.x + area.width - right).min(band_end),
            )
        }
    };
    let width = end - x;
    let centred = slots.middle.y + (slots.middle.height - height) / 2.0;
    let top = match place {
        PillPlace::Over(bottom) => bottom - height,
        _ => centred,
    };
    let mut bounds = rect(x, top, width, height);
    if let Some(scene) = slots.scene {
        if matches!(place, PillPlace::Over(_)) {
            scene.place_named("prompt pill", Vec::new(), [("prompt.asked", bounds.into())]);
        } else {
            bounds = pill_slot(scene, slots, bounds, slots.pill_width(asks));
        }
    }
    let title = text(asks).style(style).text_align(TextAlign::Center);
    let mut pill = match below {
        Some(below) => column()
            .align_items(Align::Stretch)
            .child(title.passes(true))
            .child(below),
        None => title,
    }
    .width(bounds.width)
    .min_height(height)
    .background(color::PANEL)
    // A pill of two lines keeps the ends of one, rounding only its corners.
    .radius(line / 2.0)
    .passes(true);
    pill.style.padding = [padding / 2.0, padding, padding / 2.0, padding];
    pill.anchor = Some(match place {
        PillPlace::Over(over) => Anchor::Point(Vec2::new(x, over), Vec2::Y),
        _ => Anchor::Point(Vec2::new(bounds.x, bounds.y), Vec2::ZERO),
    });
    (pill, bounds)
}

/// Tries the pill at `preferred` and then at the nearest quiet height. Off the middle band,
/// where neither the phase icon, a shared pile nor a stacked button stands, the pill takes the
/// prompt's full width. A pill narrower than `needed`, the width its words take on one line,
/// wraps, so it must be clear of cards for two lines. When cards fill the middle of the board
/// from top to bottom, the pill can use the space to either side of those cards.
fn pill_slot(scene: &PlacementScene, slots: &PromptSlots, preferred: Rect, needed: f32) -> Rect {
    let gap = slots.metrics.asked_gap;
    let height = preferred.height;
    // A narrower pill can wrap. Reserve an extra line when finding its slot.
    let tall = 2.0 * height - slots.metrics.pill_padding;
    let cards: Vec<_> = [Group::YourRows, Group::TheirRows, Group::Stack]
        .into_iter()
        .flat_map(|group| scene.group(group))
        .collect();
    let band = slots.middle;
    // Just above and below the middle band, a pill clears whatever stands on the band's line.
    let mut tops = vec![preferred.y];
    tops.extend(
        [band.y - gap - height, band.y + band.height + gap]
            .into_iter()
            .chain(
                cards
                    .iter()
                    .flat_map(|card| [card.y + card.height + gap, card.y - gap - height]),
            )
            .filter(|&top| top >= slots.clearance.y && top + height <= slots.strip_top()),
    );
    tops.sort_by(|a, b| (a - preferred.y).abs().total_cmp(&(b - preferred.y).abs()));
    let full_width = tops.into_iter().map(|y| {
        let in_band = y < band.y + band.height && y + height > band.y;
        let bounds = if in_band {
            Rect { y, ..preferred }
        } else {
            rect(slots.bar.x, y, slots.bar.width, height)
        };
        let high = if bounds.width >= needed { height } else { tall };
        Candidate {
            bounds,
            clearance: rect(slots.bar.x, y - gap, slots.bar.width, high + 2.0 * gap),
        }
    });

    let end = preferred.x + preferred.width;
    let (left, right) = cards
        .iter()
        .filter(|card| {
            card.y < preferred.y + tall + gap && card.y + card.height > preferred.y - gap
        })
        .fold((end, preferred.x), |(left, right), card| {
            (left.min(card.x), right.max(card.x + card.width))
        });
    let start = preferred.x;
    let room = [((right + gap).max(start), end), (start, left - gap)];
    let beside = room
        .into_iter()
        .filter(|(left, right)| right - left >= slots.metrics.button_min_width)
        .map(|(left, right)| {
            let bounds = rect(left, preferred.y, right - left, height);
            let clearance = inset(
                Rect {
                    height: tall,
                    ..bounds
                },
                -gap,
            );
            Candidate {
                bounds,
                clearance: clearance.clearance,
            }
        });
    scene
        .place_named(
            "prompt pill",
            table_rows(),
            full_width
                .chain(beside)
                .map(|candidate| ("prompt.bar", candidate)),
        )
        .map_or(preferred, |(_, bounds)| bounds)
}
