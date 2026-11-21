//! The prompt shape `Divide`: an amount divided among recipients, such as damage among a
//! spell's targets.
//!
//! The session opens the prompt with the least share on each recipient. Over each recipient
//! the flat layer shows its share, between a button that gives it one more and one that takes
//! one away, as the web interface's `Shares` (`ui/src/stage/Choose.tsx`) draws them (on the
//! phone the buttons stand beside the card instead, `edge_share`); each
//! sends the session `Click::Number` with the share it asks for, and the session's reply
//! brings the division as it now stands. The prompt bar says how much is not yet given out.
//! Each recipient wears the target mark the session sends, with an arrow to it from the spell
//! or ability that asks, which the client begins and ends itself (`own`).

use super::Divide;

use beryl_protocol::prompt::{Click, PromptView, ShapeBody, Share};
use beryl_protocol::protocol::Side;
use beryl_protocol::script::Subject;
use glam::Vec2;

use crate::hud::HudIn;
use crate::hud::placement_scene::{Group, PlacementScene, inset, rect};
use crate::prompts::feature::Details;
use kit_base::tokens::{color, size, space, type_style};
use kit_frame::frame::ElementId;
use kit_scene::{
    Slot,
    hit::{contains, intersection},
};
use kit_ui::element::{Align, Anchor, Element, TextAlign, column, row, text};
use kit_ui::input::Rect;
use kit_ui::widgets::{icon_button, icons};

/// What the flat layer shows for `prompt`: the prompt bar, with how much is not yet given out
/// after what it asks, as the web's `Asks` (`ui/src/stage/PromptBar.tsx`) writes it, and the
/// share of each recipient that is an object. A share is laid out at its recipient, out of the
/// bar's flow (`layout::anchor`).
pub(super) fn view(prompt: &PromptView, input: &HudIn, bar: Element) -> Element {
    let ShapeBody::Divide { left, shares, .. } = &prompt.body else {
        return bar;
    };
    let left = *left;
    let [edge, side] = input.profile.slots(["divide.edge", "divide.side"]);
    let mut steppers = Vec::new();
    let controls = shares.iter().filter_map(|&share| {
        if let Some(edge) = edge {
            edge_share(share, left, input, (edge, side), &mut steppers)
        } else {
            share_control(share, left)
        }
    });
    bar.children(controls.flatten())
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
/// (`checks.target_points` in the phone profile) and still leave a part of itself that size free to press. So
/// the column of the button that gives one more over the one that takes one away, at an icon
/// button's full size, stands against the card's edge outside it, and the share stands on the
/// card just inside that edge, on a strip low enough to leave the rest of the card free. Both
/// go above a card the opponent controls, at the top of the phone's table, and below one the
/// viewer controls, towards the window's foot, so that neither stands over the middle of the
/// table, where the prompt bar is.
fn edge_share(
    share: Share,
    left: u8,
    input: &HudIn,
    (safe, side): (Slot, Option<Slot>),
    steppers: &mut Vec<Rect>,
) -> Option<Vec<Element>> {
    let Subject::Object { slot } = share.on else {
        return None;
    };
    let theirs = opponent_controls(input, slot);
    let target = input.profile.data().checks.target_points;
    let (more, less) = steps(share, left, target);
    let name = format!("share {}", slot.0);
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
    if let Some(bounds) = side.and_then(|side| beside(input, slot, side, (target, high), steppers))
    {
        steppers.push(bounds);
        let mut control = row()
            .name(&name)
            .gap(space::S)
            .align_items(Align::Center)
            .child(more)
            .child(shown.width(high))
            .child(less);
        control.anchor = Some(Anchor::Point(Vec2::new(bounds.x, bounds.y), Vec2::ZERO));
        return Some(vec![control]);
    }
    let mut control = column().name(&name).gap(space::S).child(more).child(less);
    let above = input
        .placement_scene
        .and_then(|layout| {
            let card = layout.bounds(ElementId(u32::from(slot.0)))?;
            let height = 2.0 * target + space::S;
            let x = card.x + (card.width - target) / 2.0;
            let above = rect(x, card.y - height, target, height);
            let below = rect(x, card.y + card.height, target, height);
            let preferred = if theirs {
                [above, below]
            } else {
                [below, above]
            };
            let overlap = input
                .profile
                .data()
                .hud
                .floating_prompt
                .map_or(0.0, |metrics| metrics.overlap);
            let candidates = preferred
                .into_iter()
                .filter(|&bounds| contains(safe.rect, bounds))
                .map(|bounds| inset(bounds, overlap));
            let (_, placed) = layout.place("divide stepper", occupied(), candidates)?;
            Some(placed.y < card.y)
        })
        .unwrap_or(theirs);
    (control.anchor, shown.anchor) = if above {
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

/// The slot reserves room for a full-size stepper alongside a recipient.
fn beside(
    input: &HudIn,
    object: beryl_protocol::protocol::ObjectId,
    safe: Slot,
    (target, label): (f32, f32),
    steppers: &[Rect],
) -> Option<Rect> {
    let layout = input.placement_scene?;
    let card = layout.bounds(ElementId(u32::from(object.0)))?;
    let width = 2.0 * (target + space::S) + label;
    let centre = card.y + (card.height - target) / 2.0;
    let tops = [centre, card.y, card.y + card.height - target];
    let candidates = [card.x + card.width + space::S, card.x - width - space::S]
        .into_iter()
        .flat_map(|x| tops.map(|top| rect(x, top, width, target)))
        .filter(|&bounds| {
            contains(safe.rect, bounds)
                && steppers
                    .iter()
                    .all(|&placed| intersection(placed, bounds).is_none())
        })
        .map(Into::into);
    let (_, bounds) = layout.place("divide stepper", occupied(), candidates)?;
    Some(bounds)
}

fn occupied() -> Vec<kit_scene::NodeQuery> {
    vec![
        Group::YourRows.children(),
        Group::TheirRows.children(),
        Group::Stack.children(),
        PlacementScene::phase_query(),
    ]
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
            Some(n) => button.on_press(Divide::message(Click::Number { n, on: Some(on) })),
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

fn opponent_controls(input: &HudIn<'_>, slot: beryl_protocol::protocol::ObjectId) -> bool {
    (input.view)
        .and_then(|view| view.objects.iter().find(|object| object.slot == slot))
        .is_some_and(|object| object.controller == Side::Opponent)
}

pub(super) fn details(input: &HudIn<'_>) -> Details {
    let shares = match input.prompt.map(|prompt| &prompt.body) {
        Some(ShapeBody::Divide { shares, .. }) => shares.as_slice(),
        _ => &[],
    };
    Details::Divide(
        shares
            .iter()
            .map(|share| match share.on {
                Subject::Object { slot } => opponent_controls(input, slot),
                _ => false,
            })
            .collect(),
    )
}
