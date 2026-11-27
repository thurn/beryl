//! A prompt that picks cards in the viewer's hand, on the desktop: what it asks is written across
//! the middle of the table and its buttons stand in the dock in the window's bottom right corner,
//! as Arena lays out "Put an artifact, creature, enchantment, or land card onto the battlefield?"
//! and "Discard a card." (`3SEgIYXJgdQ 40:41.5`, `sybcO8S7x6A 26:34.5`, `nBj2ikeYCtk 17:02.5`,
//! `5UHzpVvUkfI 41:56.5`, `3sEoP_s4DQE 39:01.5`). The hand stays where it is, so nothing of the
//! prompt stands over a card the player may pick.

use beryl_protocol::PlayerView;
use beryl_protocol::prompt::{ButtonId, PromptButton, PromptView, Purpose, ShapeBody};
use beryl_protocol::protocol::{Side, ZoneName};
use beryl_protocol::script::Subject;
use glam::Vec2;

use super::Message;
use super::view::{Controls, captioned_dock, dock_sized, subtitle, turn_action};
use crate::hud::HudIn;
use crate::prompts::bar_buttons;
use game_table::profiles::Profile;
use kit_base::data::profiles::{Heading, SlotName};
use kit_base::tokens::{color, gradient, shadow, type_style};
use kit_text::text::TypeStyle;
use kit_ui::element::{Align, Anchor, Element, Justify, TextAlign, column, holder, text};
use kit_ui::layout::{DOCK_BOTTOM, DOCK_RIGHT};
use kit_ui::widgets::{button, primary};

/// The profile's heading, where `prompt` picks only cards in the viewer's hand and holds none
/// up in a tray: a discard, or a card put from the hand onto the battlefield.
pub(super) fn hand_heading(prompt: &PromptView, input: &HudIn) -> Option<Heading> {
    let heading = input.profile.data().hud.heading?;
    let view = input.view?;
    picks_in_hand(prompt, view).then_some(heading)
}

/// Whether every card `prompt` marks lies in the viewer's own hand, with at least one marked,
/// while it holds no cards up. Priority marks the cards the player may play, and the cards put
/// on the bottom after a mulligan are picked on the opening's own screen, so neither counts.
pub(super) fn picks_in_hand(prompt: &PromptView, view: &PlayerView) -> bool {
    let ShapeBody::Pick { tray, .. } = &prompt.body else {
        return false;
    };
    if prompt.is(Purpose::Priority) || prompt.is(Purpose::PutOnBottom) {
        return false;
    }
    let in_hand = |on: &Subject| {
        let Subject::Object { slot } = *on else {
            return false;
        };
        (view.objects.iter()).any(|object| {
            object.slot == slot && object.zone == ZoneName::Hand && object.owner == Side::You
        })
    };
    tray.is_empty()
        && !prompt.marks.is_empty()
        && prompt.marks.iter().all(|marked| in_hand(&marked.on))
}

/// The prompt bar for a prompt that picks in the hand: the heading with what it asks, `asks`,
/// and the dock with its buttons. Submit stands at the dock's foot, where Arena's stands, and
/// each other button over it, the first lowest; each wears the face the session gives it, so a
/// Decline put forward while Submit can't be pressed stands out over it, as Arena's does.
pub(super) fn bar(
    prompt: &PromptView,
    asks: &str,
    input: &HudIn,
    controls: Option<Controls>,
    heading: Heading,
) -> Element {
    let buttons: Vec<&PromptButton> = bar_buttons(prompt).collect();
    let foot = (buttons.iter().copied())
        .find(|button_view| button_view.id == ButtonId::Confirm)
        .or_else(|| {
            buttons
                .iter()
                .copied()
                .find(|button_view| button_view.primary)
        });
    let is_foot = |button_view: &PromptButton| foot.is_some_and(|foot| foot.id == button_view.id);
    let face = |button_view: &PromptButton| {
        let widget = if button_view.primary { primary } else { button };
        dock_sized(
            widget(&button_view.label)
                .enabled(button_view.enabled)
                .on_press(Message::Button(button_view.id)),
        )
    };
    let others: Vec<Element> = (buttons.iter().copied())
        .filter(|button_view| !is_foot(button_view))
        .rev()
        .map(face)
        .collect();
    let mut dock = captioned_dock()
        .children(others)
        .children(controls.map(|controls| controls.element))
        .children(foot.map(|foot| turn_action(face(foot), None, &foot.caption)));
    dock.anchor = Some(Anchor::Point(dock_corner(input.profile), Vec2::ONE));
    holder()
        .name("prompt")
        .children((!asks.is_empty()).then(|| line(asks, prompt, input.profile, heading)))
        .child(dock)
}

/// The dock's bottom right corner, where the desktop's `prompt.bar` slot ends.
fn dock_corner(profile: Profile) -> Vec2 {
    if let Some(slot) = profile.slot(SlotName::PromptBar) {
        let bounds = slot.rect;
        return Vec2::new(bounds.x + bounds.width, bounds.y + bounds.height);
    }
    let window = profile.window();
    Vec2::new(
        window.size.width - window.safe_area.right - DOCK_RIGHT,
        window.size.height - window.safe_area.bottom - DOCK_BOTTOM,
    )
}

/// The heading: what the prompt asks, `asks`, centred on the line across the middle of the
/// table, over a soft dark band, with the prompt's subtitle under it where it has one. It lets
/// the pointer through, so it never covers a card the prompt picks.
fn line(asks: &str, prompt: &PromptView, profile: Profile, heading: Heading) -> Element {
    let window = profile.window();
    let left = window.safe_area.left;
    let top = window.safe_area.top;
    let width = window.size.width - left - window.safe_area.right;
    let height = window.size.height - top - window.safe_area.bottom;
    let style = TypeStyle {
        size: heading.text * height,
        ..type_style::TITLE
    };
    let band_width = heading.width * width;
    let band_height = heading.band * height;
    let words = text(asks)
        .style(style)
        .color(color::QUESTION_TITLE)
        .text_align(TextAlign::Center)
        .drop_shadow(shadow::QUESTION_TITLE.to_vec())
        .passes(true);
    let below = subtitle(
        prompt,
        style.size * type_style::BODY.size / type_style::TITLE.size,
    );
    let mut band = column()
        .name("prompt heading")
        .align_items(Align::Center)
        .justify_content(Justify::Center)
        .width(band_width)
        .min_height(band_height)
        .background_image(vec![gradient::PROMPT_HEADING_BAND])
        .child(words)
        .children(below)
        .passes(true);
    band.anchor = Some(Anchor::Point(
        Vec2::new(left + width / 2.0, top + heading.line * height),
        Vec2::splat(0.5),
    ));
    band
}
