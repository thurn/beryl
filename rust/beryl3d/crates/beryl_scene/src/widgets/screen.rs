//! Panels and action buttons that fill a window or its safe area.

use glam::Vec2;

use crate::element::{Align, Anchor, Element, Overflow, TextAlign, column, row, text};
use crate::input::{Insets, Size};
use crate::tokens::{color, size, space, type_style};

/// The dimmed table, `.overlay-backdrop`: the whole window in the backdrop's color, which the
/// pointer goes through. The opening and game over draw it under what they show.
#[must_use]
pub(crate) fn backdrop(window: Size) -> Element {
    let mut backdrop = row()
        .name("scrim")
        .width(window.width)
        .height(window.height)
        .background(color::BACKDROP)
        .passes(true);
    backdrop.anchor = Some(Anchor::Point(Vec2::ZERO, Vec2::ZERO));
    backdrop
}

/// `screen` laid over the whole window, as the web's desktop screens fill the page: the flat
/// layer's root puts a screen in its bottom right corner inside the game screen's dock offsets,
/// which would centre what the screen holds left of and above the window's middle.
#[must_use]
pub(crate) fn whole_window(window: Size, screen: Element) -> Element {
    let mut screen = screen.width(window.width).height(window.height);
    screen.anchor = Some(Anchor::Point(Vec2::ZERO, Vec2::ZERO));
    screen
}

/// A portrait screen named `name`, which shows `content` in one column over the safe area: the
/// screen is clear and anchored over the whole window, and `content` stands inside its safe
/// area, the gutter in from its sides and foot, at most `phone::COLUMN` wide in the window's
/// middle. `content` scrolls when it is taller than that, as it is while the system keyboard
/// shortens the window, so that every action stays in reach; it is cut at the safe area, so
/// nothing scrolls under the status bar or the home indicator.
#[must_use]
pub(crate) fn phone_screen(name: &str, window: Size, safe: Insets, content: Element) -> Element {
    use crate::data::space::flat::phone::{COLUMN, GUTTER};
    let across = window.width - safe.left - safe.right - 2.0 * GUTTER;
    let extra = ((across - COLUMN) / 2.0).max(0.0);
    let mut content = content
        .grow(1.0)
        .shrink(1.0)
        .min_height(0.0)
        .align_self(Align::Stretch)
        .overflow(Overflow::Scroll);
    content.style.padding = [0.0, GUTTER + extra, GUTTER, GUTTER + extra];
    let mut screen = column()
        .name(name)
        .background([0; 4])
        .border(([0; 4], 0.0))
        .width(window.width)
        .height(window.height)
        .child(content);
    screen.style.padding = [safe.top, safe.right, safe.bottom, safe.left];
    // The flat layer's root puts a screen in its bottom right corner inside the dock's offsets,
    // so the screen is anchored at the window's top left instead.
    screen.anchor = Some(Anchor::Point(Vec2::ZERO, Vec2::ZERO));
    screen
}

/// `button` as a phone's action: as wide as its column, `phone::ACTION_HEIGHT` tall, with
/// larger type, as the web's phone dock draws its buttons (`.phone-opening .dock .button`).
#[must_use]
pub(crate) fn phone_action(mut button: Element) -> Element {
    use crate::data::space::flat::phone::{ACTION_HEIGHT, ACTION_TEXT};
    button.style.height = Some(ACTION_HEIGHT);
    button.style.text.size = ACTION_TEXT;
    button.style.min_width = None;
    button.style.align_self = Some(Align::Stretch);
    button.style.shrink = 0.0;
    button
}

/// The overlay shell over a window of `window`'s size: `title` and, if given, `instruction` over
/// `body` in the choice column, `actions` under it, and `aside` in the side column. The shell
/// covers the window and darkens the table under it.
#[must_use]
pub(crate) fn overlay(
    window: Size,
    (title, instruction): (&str, Option<&str>),
    aside: Option<Element>,
    body: Element,
    actions: Vec<Element>,
) -> Element {
    let header = column()
        .name("header")
        .width(size::OVERLAY_MAIN)
        .height(size::OVERLAY_HEADER)
        .align_items(Align::Center)
        .child(text(title).style(type_style::OVERLAY_TITLE))
        .children(instruction.map(|words| {
            text(words)
                .style(type_style::OVERLAY_INSTRUCTION)
                .text_align(TextAlign::Center)
        }));
    let main = column()
        .name("choice")
        .width(size::OVERLAY_MAIN)
        .align_items(Align::Center)
        .gap(space::L)
        .child(header)
        .child(body)
        .child(row().name("actions").gap(space::M).children(actions));
    let side = column()
        .name("side")
        .width(size::OVERLAY_SIDE)
        .align_items(Align::End)
        .children(aside);
    let mut shell = row()
        .name("overlay")
        .width(window.width)
        .height(window.height)
        .gap(size::OVERLAY_GAP)
        .align_items(Align::Start)
        .background(color::BACKDROP)
        .child(main)
        .child(side);
    shell.style.padding = [size::OVERLAY_TOP, 0.0, 0.0, size::OVERLAY_LEFT];
    shell.anchor = Some(Anchor::Point(Vec2::ZERO, Vec2::ZERO));
    shell
}
