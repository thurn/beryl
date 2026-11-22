//! Compares the retained declaration with the toolkit's default style.

use std::fmt::Debug;

use kit_base::data::profiles::FloatingPrompt;
use kit_ui::element::{Display, Element, Style};
use kit_ui::widgets::{button, primary, screen::supplied_timing};
use serde_json::{Value, json};

const TOKENS: &str = "data/design-tokens.json";

/// The parent's placement properties, including defaults that affect the solve.
#[must_use]
pub fn layout(parent: &Element) -> Value {
    let style = &parent.style;
    let policy = match style.display {
        Display::Flex => "flexbox",
        Display::Grid => "grid",
        Display::None => "flat",
    };
    json!({
        "policy": policy,
        "parent": parent.name,
        "direction": format!("{:?}", style.direction),
        "gap": style.gap,
        "wrap": style.wrap,
        "align_items": format!("{:?}", style.align_items),
        "justify_content": format!("{:?}", style.justify_content),
        "padding": style.padding,
        "grid_template_columns": format!("{:?}", style.grid_template_columns),
        "grid_template_rows": format!("{:?}", style.grid_template_rows),
    })
}

fn source(kind: &str, key: &str, file: &str) -> Value {
    json!({"kind": kind, "key": key, "file": file})
}

fn printed(value: &impl Debug) -> String {
    // A style can carry author-supplied gradients and filter chains. Report each property
    // even when its value is too long for one line of an inspection.
    const MOST: usize = 360;
    let mut value = format!("{value:?}");
    if let Some((limit, _)) = value.char_indices().nth(MOST) {
        value.truncate(limit);
        value.push('…');
    }
    value
}

/// Every declared property that differs from the toolkit default, with its supplying view,
/// profile field or widget token.
#[must_use]
#[expect(
    clippy::float_cmp,
    reason = "source attribution compares authored values exactly"
)]
pub fn styles(
    element: &Element,
    owner: &str,
    file: &str,
    floating: Option<(&FloatingPrompt, &str)>,
) -> Vec<Value> {
    let style = &element.style;
    let default = Style::default();
    let button = element.action_button.then(|| {
        let build = if element.primary { primary } else { button };
        build("")
    });
    let variant = if element.primary {
        "PRIMARY"
    } else {
        "SECONDARY"
    };
    let floating = floating.filter(|_| element.action_button);
    let mut values = Vec::new();
    macro_rules! properties {
        ($($field:ident $(=> $token:expr)?),* $(,)?) => {
            let Style { $($field: _,)* } = style;
            $(if style.$field != default.$field {
                let origin = source("view", owner, file);
                $(let origin = if button.as_ref().is_some_and(|button| style.$field == button.style.$field) {
                    source("token", $token, TOKENS)
                } else { origin };)?
                let origin = floating.and_then(|(metrics, profile)| {
                    let key = floating_key(style, metrics, stringify!($field))?;
                    Some(source("data", key, profile))
                }).unwrap_or(origin);
                let origin = if supplied_timing(element, stringify!($field)) {
                    source("data", "transition_ms", "data/beryl3d/flat.json")
                } else { origin };
                values.push(json!({
                    "property": stringify!($field),
                    "value": printed(&style.$field),
                    "source": origin,
                }));
            })*
        }
    }
    properties!(
        display, direction, gap, padding => "flat.size.BUTTON_PADDING",
        margin, grow, shrink, basis, wrap, align_items, align_self, justify_content, position,
        inset, width, height => "flat.size.BUTTON_HEIGHT",
        min_width => "flat.size.BUTTON_MIN_WIDTH", max_width, min_height, max_height,
        grid_template_columns, grid_template_rows, grid_column, grid_row, overflow, opacity,
        translation, scale, transition, opacity_transition, enter, leave, layout_slide,
        background, background_image => &format!("flat.gradient.{variant}"), background_boxes,
        mask, scroll_fade, filter, border, radius, legend,
        box_shadow => &format!("flat.shadow.{variant}"), drop_shadow,
        text => "flat.type_style.BUTTON", text_align,
        color => &format!("flat.color.{variant}_INK"), fit, truncate, passes, backdrop,
        backdrop_filter,
    );
    if let Some(border) = values
        .iter_mut()
        .find(|value| value["property"] == "border")
        && button
            .as_ref()
            .is_some_and(|button| style.border == button.style.border)
    {
        border["also_from"] = json!([
            source("token", &format!("flat.color.{variant}_RIM"), TOKENS),
            source("token", "flat.size.BORDER", TOKENS),
        ]);
    }
    values
}

fn floating_key(style: &Style, metrics: &FloatingPrompt, field: &str) -> Option<&'static str> {
    let padding = metrics.button_padding;
    match field {
        "padding"
            if style.padding.map(f32::to_bits)
                == [0.0, padding, 0.0, padding].map(f32::to_bits) =>
        {
            Some("hud.floating_prompt.button_padding")
        }
        "text" if style.text.size.to_bits() == metrics.button_text.to_bits() => {
            Some("hud.floating_prompt.button_text")
        }
        "min_width" if style.min_width == Some(metrics.button_min_width) => {
            Some("hud.floating_prompt.button_min_width")
        }
        "height" => Some("slots.prompt.dock.max_height"),
        _ => None,
    }
}
