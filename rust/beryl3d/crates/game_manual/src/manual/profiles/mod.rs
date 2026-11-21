//! Compact profile slots and layouts for manual table controls.

use beryl_protocol::protocol::ProposalView;
use game_hud::hud::placement_scene::{Group, PlacementScene, rect};
use game_table::profiles::TableBands;
use glam::Vec2;
use kit_base::data::profiles::Banded;
use kit_base::geometry::Rect;
use kit_base::tokens::{size, space, type_style};
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

/// A proposal tries the corner, then the space above the viewer’s strip.
pub(super) fn proposal_slot(
    proposals: &[ProposalView],
    corner: Rect,
    bands: TableBands,
    layout: Option<&PlacementScene>,
    target: f32,
) -> Option<Anchor> {
    let layout = layout?;
    let height = panel_height(proposals, target);
    let preferred = rect(corner.x, corner.y, corner.width, height);
    let over_hand = rect(
        corner.x,
        bands.your_strip.top - height,
        corner.width,
        height,
    );
    let choices = [preferred, over_hand].into_iter().map(Into::into);
    let avoids = vec![Group::YourRows.children(), Group::Stack.children()];
    let (_, placed) = layout.place("proposal panel", avoids, choices)?;
    Some(Anchor::Point(
        Vec2::new(corner.x + corner.width, placed.y),
        Vec2::X,
    ))
}

/// The space the title, review rows and Apply need before the panel is laid out.
fn panel_height(proposals: &[ProposalView], touch: f32) -> f32 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "proposal panels hold a short list"
    )]
    let count = |length: usize| length as f32;
    let title = type_style::BODY.size * type_style::BODY.line_height;
    2.0 * space::M
        + touch
        + proposals
            .iter()
            .map(|proposal| title + count(proposal.lines.len() + 1) * (touch + space::S) + space::M)
            .sum::<f32>()
}
