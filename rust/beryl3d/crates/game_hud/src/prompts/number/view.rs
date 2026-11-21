use crate::hud::HudIn;
use beryl_protocol::prompt::{Click, PromptView, ShapeBody};
use kit_base::tokens::{color, size, space, type_style};
use kit_text::text::TypeStyle;
use kit_ui::element::{Align, column, row, text};
use kit_ui::widgets::{icon_button, icons};

use super::Number;
use crate::prompts::bar::Controls;
pub(super) use crate::prompts::feature::bar_only as view;

/// A number's value, with what it means, between the step to the value below it and the step
/// to the value above it, as the web's `Stepper` draws it, and about how large it is drawn. A
/// step with no value to go to cannot be pressed. Nothing for any other prompt.
pub(in crate::prompts) fn stepper(prompt: &PromptView, input: &HudIn) -> Option<Controls> {
    let ShapeBody::Number {
        values,
        value,
        detail,
    } = &prompt.body
    else {
        return None;
    };
    let at = values.iter().position(|n| n == value);
    let below = at.and_then(|at| at.checked_sub(1)).map(|at| values[at]);
    let above = at.and_then(|at| values.get(at + 1).copied());
    let step = |to: Option<u8>, icon, name: &str| {
        let element = icon_button(icon, name).enabled(to.is_some());
        match to {
            Some(n) => element.on_press(Number::message(Click::Number { n, on: None })),
            None => element,
        }
    };
    let shown = column()
        .name("value")
        .align_items(Align::Center)
        .child(text(&value.to_string()).style(type_style::TITLE))
        .children(
            (detail.as_deref())
                .map(|detail| text(detail).style(type_style::BODY).color(color::TEXT_DIM)),
        );
    let element = row()
        .name("stepper")
        .gap(space::S)
        .align_items(Align::Center)
        .child(step(below, icons::MINUS, "Less"))
        .child(shown)
        .child(step(above, icons::PLUS, "More"));
    Some(Controls {
        element,
        size: input
            .profile
            .data()
            .hud
            .floating_prompt
            .map_or((0.0, 0.0), |metrics| {
                stepper_size(
                    &value.to_string(),
                    detail.as_deref(),
                    metrics.character_width,
                )
            }),
    })
}

/// About how large a number's stepper is drawn with `value` over `detail`, from the count of
/// their characters: an estimate on the generous side, as `bar::button_width` makes, which only
/// decides where the stepper stands on the phone. `detail` is a cost, each `{...}` of which is
/// one mana symbol drawn about as wide as its type is tall, and each other character of which
/// counts as wide as its type is tall too.
fn stepper_size(value: &str, detail: Option<&str>, character_width: f32) -> (f32, f32) {
    let line = |style: TypeStyle| style.size * style.line_height;
    #[expect(
        clippy::cast_precision_loss,
        reason = "a value and its cost are a few characters"
    )]
    let (value_width, detail_width) = (
        value.chars().count() as f32 * type_style::TITLE.size * character_width,
        detail.map_or(0.0, |detail| {
            let symbols = detail.split('{').skip(1).count();
            let inside: usize = (detail.split('{').skip(1))
                .map(|part| part.split('}').next().map_or(0, str::len) + 2)
                .sum();
            (detail.chars().count() - inside + symbols) as f32 * type_style::BODY.size
        }),
    );
    let shown = (
        value_width.max(detail_width),
        line(type_style::TITLE) + detail.map_or(0.0, |_| line(type_style::BODY)),
    );
    (
        2.0 * size::ICON_BUTTON + 2.0 * space::S + shown.0,
        size::ICON_BUTTON.max(shown.1),
    )
}
