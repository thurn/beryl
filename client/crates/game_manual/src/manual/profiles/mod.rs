//! Compact profile slots and layouts for manual table controls.

use game_hud::hud::placement_scene::{Group, PlacementScene, inset, rect};
use game_table::profiles::TableBands;
use glam::Vec2;
use kit_base::data::profiles::Banded;
use kit_base::geometry::Rect;
use kit_base::tokens::{size, space};
use kit_ui::element::{Anchor, Element, row};
use kit_ui::widgets::button;

use crate::features::ask::Message;

/// On a phone the field and its step buttons share a row above the two actions, and the
/// common counters take two rows of three, as in the web table's phone form.
pub(super) fn ask_dock(
    panel: Element,
    inputs: [Option<Element>; 2],
    field: Element,
    actions: [Element; 2],
    suggestions: Option<&[&str]>,
    (bands, metrics, anchor): (TableBands, &Banded, Option<Anchor>),
) -> Element {
    const SUGGESTIONS_PER_ROW: u8 = 3;
    let width = bands.right - bands.left;
    let padding = space::M;
    let gap = metrics.dock_gap;
    let high = metrics.button_height;
    let inner = width - 2.0 * (padding + size::BORDER);
    let control = |element: Element, wide: f32| element.min_width(0.0).width(wide).height(high);
    let [minus, plus] = inputs;
    let steps = minus.is_some() && plus.is_some();
    let input_width = if steps {
        inner - 2.0 * (high + gap)
    } else {
        inner
    };
    let fields = row()
        .gap(gap)
        .children(minus.map(|element| control(element, high)))
        .child(control(field, input_width))
        .children(plus.map(|element| control(element, high)));
    let columns = usize::from(SUGGESTIONS_PER_ROW);
    let width_per_column =
        (inner - (f32::from(SUGGESTIONS_PER_ROW) - 1.0) * gap) / f32::from(SUGGESTIONS_PER_ROW);
    let common = suggestions.into_iter().flat_map(|names| {
        names.chunks(columns).enumerate().map(|(line, names)| {
            let mut choices = row().gap(gap).name("Common counters");
            choices
                .children
                .extend(names.iter().enumerate().map(|(at, name)| {
                    let at = u8::try_from(line * columns + at).unwrap_or(u8::MAX);
                    control(
                        button(name).on_press(Message::Suggestion(at)),
                        width_per_column,
                    )
                }));
            choices
        })
    });
    let actions = row()
        .gap(gap)
        .children(actions.map(|element| control(element, (inner - gap) / 2.0)));
    let mut dock = panel
        .width(width)
        .padding(padding)
        .gap(gap)
        .child(fields)
        .children(common)
        .child(actions);
    dock.anchor = anchor;
    dock
}

/// Places the ask prompt and records the table objects the dock must clear.
pub(super) fn ask_anchor(
    bands: TableBands,
    metrics: &Banded,
    has_suggestions: bool,
    layout: Option<&PlacementScene>,
) -> Anchor {
    let width = bands.right - bands.left;
    let padding = space::M;
    let gap = metrics.dock_gap;
    let high = metrics.button_height;
    let extra = if has_suggestions {
        2.0 * (high + gap)
    } else {
        0.0
    };
    let height = 2.0 * (padding + size::BORDER + high) + gap + extra;
    let lower = rect(
        bands.left,
        bands.your_strip.top + bands.your_strip.height - height,
        width,
        height,
    );
    let upper = rect(bands.left, bands.opponent_strip.top, width, height);
    let middle = rect(
        bands.left,
        bands.middle.middle() - height / 2.0,
        width,
        height,
    );
    let avoids = vec![Group::YourRows.children(), Group::Stack.children()];
    let placed = layout
        .and_then(|layout| {
            layout.place(
                "manual ask prompt",
                avoids,
                [lower, middle, upper].map(Into::into),
            )
        })
        .map_or(upper, |(_, placed)| placed);
    Anchor::Point(Vec2::new(placed.x, placed.y), Vec2::ZERO)
}

/// Places Next beside the viewer's life total, raising it above the hand when the
/// fan takes up more than one row. The middle offers another slot on a crowded table.
pub(super) fn next_slot(
    preferred: Rect,
    bands: TableBands,
    layout: Option<&PlacementScene>,
) -> Rect {
    let Some(layout) = layout else {
        return preferred;
    };
    let hand_top = layout
        .group(Group::Hand)
        .iter()
        .map(|card| card.y)
        .fold(bands.hand.top, f32::min);
    let raised = rect(
        preferred.x,
        preferred.y.min(hand_top - preferred.height - space::S),
        preferred.width,
        preferred.height,
    );
    let middle = rect(
        preferred.x,
        bands.middle.middle() - preferred.height / 2.0,
        preferred.width,
        preferred.height,
    );
    let left = rect(bands.left, middle.y, middle.width, middle.height);
    let avoids = [Group::Hand, Group::YourRows, Group::TheirRows, Group::Stack]
        .map(Group::children)
        .to_vec();
    // Allow the middle button's border to cross the edge of a card by a few
    // points if rounding and perspective leave it just outside the gap.
    let choices = [
        raised.into(),
        middle.into(),
        left.into(),
        inset(middle, space::S),
        inset(left, space::S),
    ];
    layout
        .place("Next", avoids, choices)
        .map_or(raised, |(_, placed)| placed)
}

/// Finds a clear lane for the proposal panel at either edge of the battlefield. The
/// scrollable review can use a shorter slot when the full panel would cover a row. The panel also
/// keeps clear of `labels`, the boxes where state nudge buttons stand, which are not cards.
pub(super) fn proposal_slot(
    labels: &[Rect],
    preferred: Rect,
    short_height: Option<f32>,
    bands: TableBands,
    layout: &PlacementScene,
) -> Option<Rect> {
    let groups = [Group::YourRows, Group::TheirRows, Group::Stack, Group::Hand];
    let top = bands.opponent_back.top;
    let bottom = bands.your_strip.top;
    let left = bands.left + space::S;
    let right = preferred.x;
    let occupied: Vec<_> = groups
        .iter()
        .flat_map(|&group| layout.group(group))
        .collect();
    let heights = std::iter::once(preferred.height)
        .chain(short_height.filter(|&height| height < preferred.height));
    let choices = heights.flat_map(|height| {
        let centred = preferred.y + (preferred.height - height) / 2.0;
        let mut positions = vec![centred, top, bottom - height];
        for card in &occupied {
            positions.push(card.y + card.height + space::S);
            positions.push(card.y - height - space::S);
        }
        positions.sort_by(|a, b| (a - centred).abs().total_cmp(&(b - centred).abs()));
        positions.dedup();
        positions
            .into_iter()
            .filter(move |y| *y >= top && y + height <= bottom)
            .flat_map(move |y| [right, left].map(move |x| rect(x, y, preferred.width, height)))
            .filter(|panel| !labels.iter().any(|label| crosses(*panel, *label)))
            .map(Into::into)
    });
    let avoids = groups.map(Group::children).to_vec();
    layout
        .place("proposal panel", avoids, choices)
        .map(|(_, placed)| placed)
}

/// Whether the two boxes share any area.
fn crosses(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}
