//! Panels and action buttons that fill a window or its safe area.

use glam::Vec2;

use crate::element::{
    Align, Anchor, Element, Overflow, Presence, Property, TextAlign, Timing, Transition, column,
    row, text,
};
use crate::input::{Rect, Size};
use kit_base::data::profiles::ScreenMetrics;
use kit_base::data::space::flat::transition::MS;
use kit_base::geometry::Window;
use kit_base::pose::Curve;
use kit_base::tokens::{color, size, space, type_style};

/// How the game dock and other screen controls have entered and moved in the flat layer.
pub const SCREEN_TIMING: Timing = Timing::new(MS, 0.0, Curve::EASE_OUT);

pub const SCREEN_FADE: Presence = Presence {
    grouped: true,
    ..Presence::fade(SCREEN_TIMING)
};

/// A fixed overlay, such as the folded log, appears at full opacity with its contents.
pub const SCREEN_INSTANT: Presence = Presence {
    grouped: true,
    ..Presence::new(SCREEN_TIMING)
};

const SCREEN_OPACITY: &[Transition] = &[Transition::new(Property::Opacity, SCREEN_TIMING)];

/// Declares the animation used by the existing screen and sheet trees. A panel and its
/// direct children enter together, fixed overlays appear at once, and a transparent holder
/// leaves each panel inside it to enter on its own.
#[must_use]
pub fn with_transitions(mut screen: Element) -> Element {
    fn declare(element: &mut Element, in_fixed_overlay: bool) {
        let fixed = matches!(element.anchor, Some(Anchor::Point(..)));
        if element.style.enter.is_none()
            || (in_fixed_overlay && element.style.enter == Some(&SCREEN_FADE))
        {
            element.style.enter = if fixed || (in_fixed_overlay && !element.draws_nothing()) {
                Some(&SCREEN_INSTANT)
            } else if !element.draws_nothing() {
                Some(&SCREEN_FADE)
            } else {
                None
            };
        }
        if !in_fixed_overlay && element.style.transition.is_empty() {
            element.style.transition = SCREEN_OPACITY;
        }
        element.style.layout_slide.get_or_insert(&SCREEN_TIMING);
        for child in &mut element.children {
            declare(child, fixed);
        }
    }
    declare(&mut screen, false);
    screen
}

/// The dimmed table, `.overlay-backdrop`: the whole window in the backdrop's color, which the
/// pointer goes through. The opening and game over draw it under what they show.
#[must_use]
pub fn backdrop(window: Size) -> Element {
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
pub fn whole_window(window: Size, screen: Element) -> Element {
    let mut screen = screen.width(window.width).height(window.height);
    screen.anchor = Some(Anchor::Point(Vec2::ZERO, Vec2::ZERO));
    screen
}

/// The fitted column and action dock for a screen with limited horizontal space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenColumn {
    pub bounds: Rect,
    pub metrics: ScreenMetrics,
    pub target: f32,
}

impl ScreenColumn {
    /// Fills the window and scrolls the column when its contents outgrow the safe area.
    #[must_use]
    pub fn scrolling(self, name: &str, window: Window, content: Element) -> Element {
        let safe = window.safe_area;
        let bounds = self.bounds;
        let mut content = content
            .grow(1.0)
            .shrink(1.0)
            .min_height(0.0)
            .align_self(Align::Stretch)
            .overflow(Overflow::Scroll);
        content.style.padding = [
            bounds.y - safe.top,
            window.size.width - safe.right - (bounds.x + bounds.width),
            window.size.height - safe.bottom - (bounds.y + bounds.height),
            bounds.x - safe.left,
        ];
        let mut screen = column()
            .name(name)
            .background([0; 4])
            .border(([0; 4], 0.0))
            .child(content);
        screen.style.padding = [safe.top, safe.right, safe.bottom, safe.left];
        whole_window(window.size, screen)
    }

    /// Stretches one action across the column at the profile's action height.
    #[must_use]
    pub fn action(self, button: Element) -> Element {
        stretch_action(button, self.metrics.action_height, self.metrics.action_text)
    }

    /// Stacks a standalone screen's actions at the base of its dock.
    #[must_use]
    pub fn actions_at_foot(self, name: &str, buttons: Vec<Element>, dock: Rect) -> Element {
        let mut actions = column()
            .name(name)
            .gap(self.metrics.action_gap)
            .width(dock.width)
            .children(buttons.into_iter().map(|button| self.action(button)));
        actions.anchor = Some(Anchor::Point(
            Vec2::new(dock.x + dock.width / 2.0, dock.y + dock.height),
            Vec2::new(0.5, 1.0),
        ));
        actions
    }
}

/// The stretched action used by the game result and the story palette.
#[must_use]
pub fn compact_action(button: Element) -> Element {
    use kit_base::data::space::flat::phone::{ACTION_HEIGHT, ACTION_TEXT};
    stretch_action(button, ACTION_HEIGHT, ACTION_TEXT)
}

fn stretch_action(mut button: Element, height: f32, text: f32) -> Element {
    button.style.height = Some(height);
    button.style.text.size = text;
    button.style.min_width = None;
    button.style.align_self = Some(Align::Stretch);
    button.style.shrink = 0.0;
    button
}

/// The overlay shell over a window of `window`'s size: `title` and, if given, `instruction` over
/// `body` in the choice column, `actions` under it, and `aside` in the side column. The shell
/// covers the window and darkens the table under it.
#[must_use]
pub fn overlay(
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
