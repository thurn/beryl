//! The prompt shape `Split`: cards dealt out between two piles, such as a scry's top and bottom
//! of the library. The two piles are held up in the tray (`table::tray_layout`) as two boxes
//! side by side, each under the session's name for it, as the web interface's `TrayUnder`
//! (`ui/src/stage/Tray.tsx`) draws them. A click on a card sends it to the other pile, and a
//! card carried into a box is dropped there before the card it was let go in front of
//! (`Client::drop_held`). Which cards may move, and where each lies after a click, is the
//! session's: its reply is drawn.
//!
//! Cards picked out of the library from among the rest lie in one fan with no boxes, the picked
//! ones marked by the prompt. Cards put back from the hand onto the library lie as the library
//! and the hand, and their piles' boxes are drawn with their names and no edges, as the web's
//! `TrayPiles` draws them; the table draws the library's card backs under the cards on it.
//!
//! The flat layer is drawn over the table, so a box is its rim and its name alone, and lets the
//! pointer through to the cards in it: the web's translucent fill would cover them.

use beryl_protocol::prompt::{PromptView, ShapeBody};
use glam::Vec2;

use super::{Piles, Shown, Steps, prompt_bar, tray};
use crate::data::audit;
use crate::element::{Anchor, Element, TextAlign, row, text};
use crate::hud::HudIn;
use crate::table::{DESKTOP, tray_boxes};
use crate::text::TypeStyle;
use crate::tokens::{color, size, type_style};

/// The share of a box's heading its name's type takes, and the least size it is drawn at, in
/// stage pixels: `.stage-tray-label`'s `max(12px, calc(var(--heading) * 0.5))`.
const LABEL_SHARE: f32 = 0.5;
const LABEL_LEAST: f32 = size::SPLIT_LABEL_LEAST;

/// What the flat layer shows for `prompt`: the prompt bar, and a box round each pile's place in
/// the tray, its name in the strip over its cards; none for a fan, and none of their edges for
/// a put-back.
#[expect(
    clippy::unnecessary_wraps,
    reason = "every shape returns an `Option`: a built shape may show nothing in the flat layer"
)]
pub(super) fn screen(prompt: &PromptView, input: &HudIn) -> Option<Element> {
    let bar = prompt_bar(prompt, input);
    let (ShapeBody::Split { piles }, Some(held)) = (&prompt.body, Piles::of(prompt)) else {
        return Some(bar);
    };
    let space = DESKTOP.tray_for(input.window);
    let size = input.window.size;
    let per_point = space.stage_height / size.height;
    let heading = space.heading / per_point;
    let label = TypeStyle {
        weight: 600,
        // On the phone no line of text is smaller than its least size (`agent::checks`).
        size: ((space.heading * LABEL_SHARE).max(LABEL_LEAST) / per_point).max(
            if input.window.phone() {
                audit::PHONE_TEXT_POINTS
            } else {
                0.0
            },
        ),
        line_height: 1.0,
        ..type_style::BODY
    };
    let edged = !held.put_back();
    let boxes = tray_boxes(&held.held(None), space, size);
    Some(bar.children(piles.iter().zip(boxes).map(|(pile, area)| {
        let mut pile_box = row()
            .name(&pile.label)
            .width(area.width)
            .height(area.height)
            .passes(true);
        if edged {
            pile_box = (pile_box.border((color::TRAY_PILE_RIM, size::TRAY_PILE_BORDER)))
                .radius(size::TRAY_PILE_RADIUS);
        }
        pile_box = pile_box.child(
            text(&pile.label)
                .style(label)
                .color(color::GOLD)
                .text_align(TextAlign::Center)
                .grow(1.0)
                .height(heading),
        );
        pile_box.anchor = Some(Anchor::Point(Vec2::new(area.x, area.y), Vec2::ZERO));
        pile_box
    })))
}

/// Makes what the table shows for the prompt `shown` replace what it showed for `was`, as one
/// beat (`tray::own`): the marks the session sends on the cards, and the two piles held up in
/// the tray, sorted again after each reply.
pub(super) fn own(shown: Option<Shown>, was: Option<Shown>, steps: &mut Steps) {
    tray::own(shown, was, tray::marks, steps);
}
