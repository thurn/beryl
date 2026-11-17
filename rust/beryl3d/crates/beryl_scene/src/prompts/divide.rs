//! The prompt shape `Divide`: an amount divided among recipients, such as damage among a
//! spell's targets.
//!
//! The session opens the prompt with the least share on each recipient. Over each recipient
//! the flat layer shows its share, between a button that gives it one more and one that takes
//! one away, as the web interface's `Shares` (`ui/src/stage/Choose.tsx`) draws them (on the
//! phone the buttons stand beside the card instead, `phone_share`); each
//! sends the session `Click::Number` with the share it asks for, and the session's reply
//! brings the division as it now stands. The prompt bar says how much is not yet given out.
//! Each recipient wears the target mark the session sends, with an arrow to it from the spell
//! or ability that asks, which the client begins and ends itself (`own`).

use beryl_protocol::prompt::{Click, PromptView, ShapeBody, Share};
use beryl_protocol::protocol::Side;
use beryl_protocol::script::Subject;
use glam::Vec2;

use super::bar::bar;
use super::{Shown, Steps, begin_spans, end_spans, mark_spans, target_arrows};
use crate::element::{Align, Anchor, Element, Press, TextAlign, column, text};
use crate::light::Span;
use crate::screens::ScreenIn;
use crate::tokens::{color, size, space, type_style};
use crate::widgets::{icon_button, icons};

/// What the flat layer shows for `prompt`: the prompt bar, with how much is not yet given out
/// after what it asks, as the web's `Asks` (`ui/src/stage/PromptBar.tsx`) writes it, and the
/// share of each recipient that is an object. A share is laid out at its recipient, out of the
/// bar's flow (`layout::anchor`).
#[expect(
    clippy::unnecessary_wraps,
    reason = "every shape returns an `Option`: a built shape may show nothing in the flat layer"
)]
pub(super) fn screen(prompt: &PromptView, input: &ScreenIn) -> Option<Element> {
    let ShapeBody::Divide { left, shares, .. } = &prompt.body else {
        return Some(bar(prompt, input, None, None));
    };
    let left = *left;
    let controls = (shares.iter()).filter_map(|&share| {
        if input.window.phone() {
            phone_share(share, left, input)
        } else {
            share_control(share, left)
        }
    });
    Some(bar(prompt, input, Some(&left.to_string()), None).children(controls.flatten()))
}

/// The control over a recipient that is an object: a column centred on it of the button that
/// gives it one more, its share, and the button that takes one away, named "share" and the
/// recipient's slot. Its buttons are narrower than an icon button's, so that the card under
/// them keeps a strip on either side where a press reaches it (the "reachable" check). A player
/// is given no control, as the web's is not.
fn share_control(share: Share, left: u8) -> Option<Vec<Element>> {
    let Subject::Object { slot } = share.on else {
        return None;
    };
    let (more, less) = steps(share, left, size::SHARE_STEP);
    let mut control = column()
        .name(&format!("share {}", slot.0))
        .align_items(Align::Center)
        .gap(space::S)
        .child(more)
        .child(amount(share.amount))
        .child(less);
    control.anchor = Some(Anchor::Object(share.on, Vec2::splat(0.5)));
    Some(vec![control])
}

/// The phone's control for a recipient that is an object. A card on the phone's table can be
/// as small as 56 by 76 points, too small to hold buttons of the phone's touch size
/// (`audit::PHONE_TARGET_POINTS`) and still leave a part of itself that size free to press. So
/// the column of the button that gives one more over the one that takes one away, at an icon
/// button's full size, stands against the card's edge outside it, and the share stands on the
/// card just inside that edge, on a strip low enough to leave the rest of the card free. Both
/// go above a card the opponent controls, at the top of the phone's table, and below one the
/// viewer controls, towards the window's foot, so that neither stands over the middle of the
/// table, where the prompt bar is.
fn phone_share(share: Share, left: u8, input: &ScreenIn) -> Option<Vec<Element>> {
    let Subject::Object { slot } = share.on else {
        return None;
    };
    let theirs = (input.view)
        .and_then(|view| view.objects.iter().find(|object| object.slot == slot))
        .is_some_and(|object| object.controller == Side::Opponent);
    let (more, less) = steps(share, left, size::ICON_BUTTON);
    let mut control = column()
        .name(&format!("share {}", slot.0))
        .gap(space::S)
        .child(more)
        .child(less);
    // The share's strip, as high as a desktop step is wide.
    let high = size::SHARE_STEP;
    let mut shown = text(&share.amount.to_string())
        .name(&format!("share amount {}", slot.0))
        .style(type_style::BODY)
        .text_align(TextAlign::Center)
        .background(color::PANEL)
        .radius(space::S)
        .height(high)
        .min_width(high);
    (control.anchor, shown.anchor) = if theirs {
        (
            Some(Anchor::Above(share.on, 0.0)),
            Some(Anchor::Above(share.on, -high)),
        )
    } else {
        (
            Some(Anchor::Below(share.on, 0.0)),
            Some(Anchor::Below(share.on, -high)),
        )
    };
    Some(vec![control, shown])
}

/// The buttons `side` points across that give the share one more and take one away, each
/// sending the session `Click::Number` with the share it asks for. One more cannot be given
/// when nothing is left, nor one taken from nothing, as the web's `Shares` disables them.
fn steps(Share { on, amount }: Share, left: u8, side: f32) -> (Element, Element) {
    let step = |icon, name: &str, to: Option<u8>| {
        let button = icon_button(icon, name)
            .width(side)
            .height(side)
            .radius(side / 2.0)
            .enabled(to.is_some());
        match to {
            Some(n) => button.on_press(Press::Prompt(Click::Number { n, on: Some(on) })),
            None => button,
        }
    };
    (
        step(
            icons::PLUS,
            "More",
            (left > 0).then(|| amount.saturating_add(1)),
        ),
        step(icons::MINUS, "Less", amount.checked_sub(1)),
    )
}

/// A recipient's share, on a panel.
fn amount(amount: u8) -> Element {
    text(&amount.to_string())
        .style(type_style::TITLE)
        .text_align(TextAlign::Center)
        .background(color::PANEL)
        .radius(space::S)
        .padding(space::S)
        .min_width(size::SHARE_STEP)
}

/// Makes what the table shows for the division `shown` replace what it showed for `was`, as one
/// beat: the marks and arrows `was` drew and `shown` does not end, then those new in `shown`
/// begin. Allocates nothing.
pub(super) fn own(shown: Option<Shown>, was: Option<Shown>, steps: &mut Steps) {
    end_spans(spans, shown, was, steps);
    begin_spans(spans, shown, was, steps);
}

/// The spans the table draws for `prompt`, as `Prompts.#mark` does: each of its marks, and an
/// arrow to each recipient from the spell or ability that asks (`target_arrows`).
fn spans(prompt: Option<Shown<'_>>) -> impl Iterator<Item = Span> + '_ {
    prompt
        .into_iter()
        .flat_map(|shown| mark_spans(shown.prompt).chain(target_arrows(shown)))
}
