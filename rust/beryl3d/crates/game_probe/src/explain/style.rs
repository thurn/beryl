//! Explains the retained declaration using the toolkit's base and widget styles.

use std::fmt::Debug;

use kit_base::data::profiles::FloatingPrompt;
use kit_ui::element::{Display, Element, Style};
use kit_ui::widgets::{Widget, button, primary, screen::supplied_timing};
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

/// Style values that differ from the base or widget style, with their supplying widget,
/// token, profile field or view.
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
    // Build the reference styles here so the live client keeps no per-property sources.
    let ordinary_button = element.action_button.then(|| button(""));
    let primary_button = (element.action_button && element.primary).then(|| primary(""));
    let button = primary_button.as_ref().or(ordinary_button.as_ref());
    let button_file = format!("rust/beryl3d/{}", Widget::Button.source());
    let variant = if element.primary {
        "PRIMARY"
    } else {
        "SECONDARY"
    };
    let floating = floating.filter(|_| element.action_button);
    let mut values = Vec::new();
    macro_rules! properties {
        (@origin $widget:expr, $file:expr; $token:expr) => {
            source("token", $token, TOKENS)
        };
        (@origin $widget:expr, $file:expr) => {
            source("widget", $widget, $file)
        };
        ($($field:ident $(=> $token:expr)?),* $(,)?) => {
            let Style { $($field: _,)* } = style;
            $(let widget_sets_field = button.is_some_and(|button| button.style.$field != default.$field);
            if style.$field != default.$field || widget_sets_field {
                let origin = if widget_sets_field && button.is_some_and(|button| style.$field == button.style.$field) {
                    properties!(@origin
                        if ordinary_button.as_ref().is_some_and(|button| style.$field == button.style.$field) {
                            "kit_ui::widgets::button::button"
                        } else {
                            "kit_ui::widgets::button::primary"
                        },
                        &button_file $(; $token)?
                    )
                } else {
                    source("view", owner, file)
                };
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
        grid_template_columns, grid_template_rows, grid_column, grid_row, overflow, opacity, opaque_on_hover,
        translation, scale, transition, opacity_transition, enter, leave, layout_slide,
        background, background_image => &format!("flat.gradient.{variant}"), background_boxes,
        mask, scroll_fade, filter, hover_filter, border, radius, legend,
        box_shadow => &format!("flat.shadow.{variant}"), drop_shadow,
        text => "flat.type_style.BUTTON", text_align,
        color => &format!("flat.color.{variant}_INK"), fit, truncate, passes, backdrop,
        backdrop_filter,
    );
    if let Some(border) = values
        .iter_mut()
        .find(|value| value["property"] == "border")
        && border["source"]["kind"] == "widget"
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
