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
//! Cards ordered on the library in one pile, Arena's "Order Cards" ("Drag cards to order them on
//! top of your library.", `Qrd11-wXZrk 8:04.0`), lie in one box with no name over it, the
//! bottom card at the left and the top card at the right, as the session lists them. The
//! session's words for the row's ends, "Bottom" and "Top" (`words.piles`), are written under
//! the box's left and right ends, as Arena writes them.
//!
//! The flat layer is drawn over the table, so a box is its rim and its name alone, and lets the
//! pointer through to the cards in it: the web's translucent fill would cover them.

use crate::prompts::{Piles, tray};
use beryl_protocol::prompt::{PlaceName, PromptView, ShapeBody};
use glam::Vec2;

use crate::hud::HudIn;
use game_table::table::tray_boxes;
use kit_base::tokens::{color, size, type_style};
use kit_text::text::TypeStyle;
use kit_ui::element::{Anchor, Element, TextAlign, row, text};

/// The share of a box's heading its name's type takes, and the least size it is drawn at, in
/// stage pixels: `.stage-tray-label`'s `max(12px, calc(var(--heading) * 0.5))`.
const LABEL_SHARE: f32 = 0.5;
const LABEL_LEAST: f32 = size::SPLIT_LABEL_LEAST;

/// What the flat layer shows for `prompt`: the prompt bar, and a box round each pile's place in
/// the tray, its name in the strip over its cards; none for a fan, and none of their edges for
/// a put-back. A pile ordered on the library has no name over it, and the words for its row's
/// ends under it.
pub(super) fn view(prompt: &PromptView, input: &HudIn, bar: Element) -> Element {
    let (ShapeBody::Split { piles }, Some(held)) = (&prompt.body, Piles::of(prompt)) else {
        return bar;
    };
    let space = input.profile.tray();
    let size = input.window.size;
    let per_point = space.stage_height / size.height;
    let heading = space.heading / per_point;
    let label = TypeStyle {
        weight: 600,
        size: ((space.heading * LABEL_SHARE).max(LABEL_LEAST) / per_point)
            .max(input.profile.data().checks.text_points.unwrap_or(0.0)),
        line_height: 1.0,
        ..type_style::BODY
    };
    let edged = !held.put_back();
    // The one pile left to order on the library (`Pile::cards`).
    let ordered = matches!(&piles[..], [first, second] if first.closed && first.cards.is_empty()
        && matches!(second.place, Some(PlaceName::Top | PlaceName::OnTop | PlaceName::Bottom)));
    let boxes = tray_boxes(&held.held(None), space, size);
    let ends = (boxes.first().filter(|_| ordered)).map(|area| {
        let at = Vec2::new(area.x, area.y + area.height + space.gap / per_point);
        tray::ends(&prompt.words.piles, at, area.width, label)
    });
    bar.children(
        held.pile_indices()
            .iter()
            .filter_map(|&index| piles.get(index))
            .zip(boxes)
            .map(|(pile, area)| {
                let mut pile_box = row()
                    .name(&pile.label)
                    .width(area.width)
                    .height(area.height)
                    .passes(true);
                if edged {
                    pile_box = (pile_box.border((color::TRAY_PILE_RIM, size::TRAY_PILE_BORDER)))
                        .radius(size::TRAY_PILE_RADIUS);
                }
                if !ordered {
                    pile_box = pile_box.child(
                        text(&pile.label)
                            .style(label)
                            .color(color::GOLD)
                            .text_align(TextAlign::Center)
                            .grow(1.0)
                            .height(heading),
                    );
                }
                pile_box.anchor = Some(Anchor::Point(Vec2::new(area.x, area.y), Vec2::ZERO));
                pile_box
            }),
    )
    .children(ends)
}
