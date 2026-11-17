//! The prompt bar: what the open prompt asks and its buttons, each as the session sent it
//! (docs/beryl-3d.md, "Elements"). Its words, which buttons it has and whether each can be
//! pressed are the session's.

use std::borrow::Cow;

use beryl_protocol::prompt::{ButtonId, PromptButton, PromptView};
use beryl_protocol::protocol::Status;
use glam::Vec2;

use super::Piles;
use crate::data::space::hud::phone as data;
use crate::element::{Align, Anchor, Element, Press, TextAlign, column, holder, row, text};
use crate::hud::phone::asked_top;
use crate::screens::ScreenIn;
use crate::table::phone_bands;
use crate::text::TypeStyle;
use crate::tokens::{color, space, type_style};
use crate::widgets::{button, icon_button, icons, primary};

/// The prompt bar for `prompt`: its title, then its buttons in the order the session sent them,
/// the one the prompt puts forward drawn as the primary button. The ways to use a card stand
/// over the table instead (`super::ways`).
/// A prompt with no title, such as a priority stop's, shows only its buttons, as the web
/// interface's prompt bar does (`ui/src/stage/PromptBar.tsx`): an empty text would still take
/// a gap beside them.
///
/// While the viewer controls the opponent's turn, a decision they make for the opponent says
/// "For opponent" before what it asks, and a priority says only that, as the web's `Asks` does
/// ([CR 724.5]): "While controlling another player, a player makes all choices and decisions
/// the controlled player is allowed to make or is told to make by the rules or by any objects."
///
/// [CR 724.5]: https://yawgatog.com/resources/magic-rules/#R7245
#[must_use]
pub fn prompt_bar(prompt: &PromptView, input: &ScreenIn) -> Element {
    bar(prompt, input, None, None)
}

/// The prompt bar for `prompt`, as `prompt_bar` draws it, with `after` written in brackets
/// after what it asks, as the web's `Asks` writes how much of a cost is still to pay, and
/// `controls` just before the primary button, as the web's dock stands a number's steps
/// (`Stepper` in `ui/src/stage/PromptBar.tsx`), or after the buttons where none is primary.
pub(super) fn bar(
    prompt: &PromptView,
    input: &ScreenIn,
    after: Option<&str>,
    controls: Option<Element>,
) -> Element {
    let acting_for = input.view.is_some_and(|view| {
        matches!(&view.status, Status::Deciding { decision } if decision.acting_for.is_some())
    });
    let asks: Cow<str> = match (acting_for, prompt.title.is_empty()) {
        (false, _) => Cow::Borrowed(&prompt.title),
        (true, true) => Cow::Borrowed("For opponent"),
        (true, false) => Cow::Owned(format!("For opponent: {}", prompt.title)),
    };
    let asks = match after {
        Some(after) => Cow::Owned(format!("{asks} ({after})")),
        None => asks,
    };
    if input.window.phone() {
        return phone_bar(prompt, input, &asks, controls);
    }
    let title = (!asks.is_empty()).then(|| text(&asks).style(type_style::TITLE).grow(1.0));
    let ways = || super::ways(prompt).map(|way| way.id);
    let buttons =
        (prompt.buttons.iter()).filter(|button_view| !ways().any(|id| id == button_view.id));
    let primary_at = buttons.clone().position(|button_view| button_view.primary);
    let mut controls = controls;
    let mut bar = row()
        .name("prompt")
        .gap(space::M)
        .padding(space::L)
        .children(title);
    for (at, button_view) in buttons.enumerate() {
        if Some(at) == primary_at {
            bar.children.extend(controls.take());
        }
        let widget = if button_view.primary { primary } else { button };
        bar.children.push(
            widget(&button_view.label)
                .enabled(button_view.enabled)
                .on_press(Press::Button(button_view.id)),
        );
    }
    bar.children.extend(controls);
    bar
}

/// The prompt bar on the phone layout, as the web interface's `PhonePromptDock` and
/// `PhonePromptOver` (`ui/src/stage/PromptBar.tsx`) compose it from the same buttons:
///
/// - The button the prompt puts forward stands at the right of the viewer's strip, where the
///   turn button is, with the one that passes on beside it. When the viewer's hand holds more
///   cards than one fan has room for, its back row stands in the strip, so the dock rises
///   above the hand's top and leaves each card a strip to press.
/// - Its other buttons, and a number's steps, are stacked over the dock. While the prompt holds
///   cards up and puts no button forward, they stand on the dock's line instead, so that they
///   stay clear of the cards the tray lays out over the viewer's rows (`table::phone::tray`).
/// - What it asks, `asks`, is a pill on the middle band's line, as Arena writes it across the
///   middle of its table, or at the nearest height clear of every card where one lies on that
///   line (`hud::phone::asked_top`). While the prompt holds cards up, or lists what it asks
///   about over the table, the table lies under a shade and the pill stands over the strip,
///   beside the stacked buttons. The pill lets the pointer through, so it never covers a card
///   the prompt picks.
fn phone_bar(
    prompt: &PromptView,
    input: &ScreenIn,
    asks: &str,
    controls: Option<Element>,
) -> Element {
    let bands = phone_bands(input.window).unwrap_or_default();
    let ways = || super::ways(prompt).map(|way| way.id);
    let buttons: Vec<&PromptButton> = (prompt.buttons.iter())
        .filter(|button_view| !ways().any(|id| id == button_view.id))
        .collect();
    let forward = buttons
        .iter()
        .copied()
        .find(|button_view| button_view.primary);
    let pass_on = (buttons.iter().copied())
        .find(|button_view| button_view.id == ButtonId::PassOn && !button_view.primary);
    let others = (buttons.iter().copied())
        .filter(|button_view| Some(button_view.id) != forward.map(|b| b.id))
        .filter(|button_view| Some(button_view.id) != pass_on.map(|b| b.id));
    let gap = data::DOCK_GAP;
    let height = data::BUTTON_HEIGHT;
    let strip = bands.your_strip;
    let mut middle = strip.middle();
    if let Some(hand_top) = input.occupied.hand_top {
        middle = middle.min(hand_top - gap - height / 2.0);
    }
    let dock = (forward.is_some() || pass_on.is_some()).then(|| {
        let mut dock = row()
            .name("dock")
            .gap(gap)
            .align_items(Align::Center)
            .children(forward.map(phone_button))
            .children(pass_on.map(|button_view| {
                icon_button(icons::PASS_ON, &button_view.label)
                    .enabled(button_view.enabled)
                    .on_press(Press::Button(button_view.id))
            }));
        dock.anchor = Some(Anchor::Point(
            Vec2::new(bands.right, middle),
            Vec2::new(1.0, 0.5),
        ));
        dock
    });
    let held_up = Piles::of(prompt).is_some();
    let over = if held_up && forward.is_none() && pass_on.is_none() {
        middle + height / 2.0
    } else {
        (middle - height / 2.0).min(strip.top) - gap
    };
    let mut more = column()
        .name("more")
        .gap(gap)
        .align_items(Align::Stretch)
        .children(controls)
        .children(others.map(phone_button));
    more.anchor = Some(Anchor::Point(
        Vec2::new(bands.right, over),
        Vec2::new(1.0, 1.0),
    ));
    let more = (!more.children.is_empty()).then_some(more);
    let held = held_up || super::covers_table(prompt);
    let pill = (!asks.is_empty()).then(|| {
        let style = TypeStyle {
            size: data::PILL_TEXT,
            ..type_style::TITLE
        };
        let padding = data::PILL_PADDING;
        let pill_height = style.size * style.line_height + padding;
        let width = bands.right - bands.left;
        // A fixed width, so that a long question is laid out on as many lines as it takes.
        let mut pill = text(asks)
            .style(style)
            .text_align(TextAlign::Center)
            .width(if held {
                width - data::BUTTON_MIN_WIDTH - gap
            } else {
                width
            })
            .min_height(pill_height)
            .background(color::PANEL)
            .radius(pill_height / 2.0)
            .passes(true);
        pill.style.padding = [padding / 2.0, padding, padding / 2.0, padding];
        pill.anchor = Some(if held {
            Anchor::Point(Vec2::new(bands.left, over), Vec2::new(0.0, 1.0))
        } else {
            let top = asked_top(&bands, input.occupied, pill_height);
            Anchor::Point(
                Vec2::new(f32::midpoint(bands.left, bands.right), top),
                Vec2::new(0.5, 0.0),
            )
        });
        pill
    });
    // The pill comes first, so that the buttons are drawn over it where they meet.
    holder()
        .name("prompt")
        .children(pill)
        .children(more)
        .children(dock)
}

/// One of a prompt's buttons on the phone, as large as a finger (`.phone-dock .button`): the
/// primary one drawn as the primary button.
fn phone_button(button_view: &PromptButton) -> Element {
    let widget = if button_view.primary { primary } else { button };
    let mut element = widget(&button_view.label)
        .enabled(button_view.enabled)
        .on_press(Press::Button(button_view.id))
        .height(data::BUTTON_HEIGHT)
        .min_width(data::BUTTON_MIN_WIDTH)
        .style(TypeStyle {
            size: data::BUTTON_TEXT,
            ..type_style::BUTTON
        });
    let padding = data::BUTTON_PADDING;
    element.style.padding = [0.0, padding, 0.0, padding];
    element
}
