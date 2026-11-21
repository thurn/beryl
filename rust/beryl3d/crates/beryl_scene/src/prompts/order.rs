//! The prompt shape `Order`: waiting triggered abilities put in the order they will resolve.
//! The session lists them first to last; the flat layer holds them up in a row over the shade
//! across the tray's space, the first to resolve at the right and the last at the left, each
//! end under the session's word for it (`words.piles`), as the web interface's `OrderRow`
//! (`ui/src/stage/Tray.tsx`) draws them. A waiting trigger is not on the stack yet, so it is no
//! scene object: each is drawn flat with the face its ability has on the stack, the plate over
//! its source's picture.
//!
//! A click on one puts it first, which the session hears as a drop at the front of the row
//! (`Click::Drop` with `at` 0), as the web's click does: the session's order prompt takes no
//! other click on an item. One carried along the row and let go is dropped before the item it
//! lands in front of (`slot_at`, `Client::carry_item`). The order after each is the session's:
//! its reply is drawn.

use beryl_protocol::prompt::{Click, PromptView, ShapeBody};
use beryl_protocol::protocol::{Candidate, CandidateView, PlayerView, Status};
use beryl_protocol::script::Subject;
use glam::Vec2;

use super::{Shown, Steps, prompt_bar};
use crate::hud::HudIn;
use game_table::faces::FaceList;
use game_table::table::{CARD_HEIGHT, CARD_WIDTH, TraySpace, tray_area};
use kit_base::tokens::{color, size, space, type_style};
use kit_frame::frame::Role;
use kit_text::text::TypeStyle;
use kit_ui::element::{Anchor, Element, Justify, Shown as Showing, row, text};
use kit_ui::input::{FaceSize, Rect, Size};

/// The space between the row and the words under it, their type's size and weight, and the
/// widest they spread, in stage pixels: `.stage-order`'s `gap` and `.stage-order-ends`' `font`
/// and `width` in `ui/src/stage/stage.css`.
const ENDS_GAP: f32 = space::M;
const ENDS_SIZE: f32 = size::ORDER_ENDS_SIZE;
const ENDS_WEIGHT: u16 = 600;
const ENDS_MOST: f32 = size::ORDER_ENDS_MOST;

/// The tray's space on the desktop layout, where the row lies.
const SPACE: TraySpace = TraySpace::DESKTOP;

/// What the flat layer shows for `prompt`: the shade over the table, the waiting triggers in
/// their row with the session's words for its two ends under it, and the prompt bar. The item
/// `carried`, which the pointer carries along the row, is left out of it: a float draws it
/// under the pointer (`carried_item`).
#[expect(
    clippy::unnecessary_wraps,
    reason = "every shape returns an `Option`: a built shape may show nothing in the flat layer"
)]
pub(super) fn screen(prompt: &PromptView, input: &HudIn) -> Option<Element> {
    let bar = prompt_bar(prompt, input);
    let ShapeBody::Order { items } = &prompt.body else {
        return Some(bar);
    };
    if items.is_empty() {
        return Some(bar);
    }
    let size = input.window.size;
    let mut shade = row()
        .name("shade")
        .width(size.width)
        .height(size.height)
        .background(color::TRAY_SHADE)
        .passes(true);
    shade.anchor = Some(Anchor::Point(Vec2::ZERO, Vec2::ZERO));
    let row_items = (items.iter().zip(cells(items.len(), size)).enumerate())
        .filter(|(_, (on, _))| input.carried != Some(**on))
        .map(|(place, (&on, cell))| {
            let mut element = item((on, place), input.view, input.faces, cell);
            element.anchor = Some(Anchor::Point(Vec2::new(cell.x, cell.y), Vec2::ZERO));
            element
        });
    Some(
        row()
            .name("order")
            .child(shade)
            .children(row_items)
            .child(ends(prompt, items.len(), size))
            .child(bar),
    )
}

/// The waiting trigger `on`, `place` in the order from the first to resolve, as the row draws
/// it in `cell`: the plate face of its ability, or its source's face where its words are not a
/// paragraph of its source's text. It is named for its place and its source, as in
/// "1. Currency Converter", since two triggers of one source are often ordered. A press puts
/// it first.
fn item(
    (on, place): (Subject, usize),
    view: Option<&PlayerView>,
    faces: Option<&FaceList>,
    cell: Rect,
) -> Element {
    let candidate = candidate(on, view);
    let source = candidate.and_then(|candidate| match candidate.option {
        Candidate::Trigger { source, .. } => source,
        _ => None,
    });
    let source_name = source
        .and_then(|slot| view?.objects.iter().find(|object| object.slot == slot))
        .and_then(|object| object.name.as_deref());
    let name = source_name.or(candidate.map(|candidate| candidate.label.as_str()));
    let face = name.zip(faces).and_then(|(name, faces)| {
        let candidate = candidate?;
        let plate_face = (candidate.text.is_none())
            .then(|| faces.plate(name, true, candidate.nth.unwrap_or(0)))
            .flatten();
        plate_face.or_else(|| faces.find(name))
    });
    Element {
        shows: face.map(|face| Showing::Face(face, FaceSize::Board)),
        ..Element::new(Role::Ability)
    }
    .name(&format!("{}. {}", place + 1, name.unwrap_or("")))
    .width(cell.width)
    .height(cell.height)
    .radius(size::ORDER_ITEM_RADIUS)
    .on_press(Click::Drop { on, pile: 0, at: 0 })
}

/// The open decision's candidate that the item `on` of an order names: the order's items are
/// abilities numbered by their candidate's index.
fn candidate(on: Subject, view: Option<&PlayerView>) -> Option<&CandidateView> {
    let Subject::Ability { id } = on else {
        return None;
    };
    match &view?.status {
        Status::Deciding { decision } => decision.candidates.get(usize::try_from(id).ok()?),
        _ => None,
    }
}

/// The session's words for the row's two ends, under it: the last to resolve at the left and
/// the first at the right.
fn ends(prompt: &PromptView, count: usize, size: Size) -> Element {
    let [last, first] = &prompt.words.piles;
    let (per_point, area) = (per_point(size), tray_area(SPACE, size));
    let style = TypeStyle {
        weight: ENDS_WEIGHT,
        size: ENDS_SIZE / per_point,
        line_height: 1.0,
        ..type_style::BODY
    };
    let width = area.width.min(ENDS_MOST / per_point);
    let word = |words: &str| text(words).style(style).color(color::GOLD);
    let mut ends = row()
        .name("ends")
        .width(width)
        .justify_content(Justify::SpaceBetween)
        .child(word(last))
        .child(word(first));
    let row_bottom = row_top(count, size) + item_width(count, size) * CARD_HEIGHT / CARD_WIDTH;
    let top = row_bottom + ENDS_GAP / per_point;
    ends.anchor = Some(Anchor::Point(
        Vec2::new(area.x + (area.width - width) / 2.0, top),
        Vec2::ZERO,
    ));
    ends
}

/// Logical points to stage pixels on a window `size`.
fn per_point(size: Size) -> f32 {
    SPACE.stage_height / size.height
}

/// How wide each of `count` items is drawn, in logical points, as `OrderRow` sizes them: the
/// space's width shared out with a gap beside each, kept between the least and the most a
/// card in a tray is drawn.
#[expect(clippy::cast_precision_loss, reason = "a row holds a few items")]
fn item_width(count: usize, size: Size) -> f32 {
    let per_point = per_point(size);
    let wide = tray_area(SPACE, size).width * per_point;
    let gap = SPACE.gap;
    let shared = (wide - gap) / count.max(1) as f32 - gap;
    SPACE.least.max(SPACE.most.min(shared)) / per_point
}

/// The top of the row and the words under it, centred together in the space, in logical
/// points.
fn row_top(count: usize, size: Size) -> f32 {
    let per_point = per_point(size);
    let area = tray_area(SPACE, size);
    let tall =
        item_width(count, size) * CARD_HEIGHT / CARD_WIDTH + (ENDS_GAP + ENDS_SIZE) / per_point;
    area.y + (area.height - tall) / 2.0
}

/// Where each of `count` items lies in the row on a window `size`, in logical points, in the
/// order the prompt lists them: the first to resolve at the right and each after it to its
/// left, the row centred in the space, as `.stage-order-row`'s `row-reverse` lays them out.
#[expect(clippy::cast_precision_loss, reason = "a row holds a few items")]
pub(crate) fn cells(count: usize, size: Size) -> impl Iterator<Item = Rect> {
    let area = tray_area(SPACE, size);
    let width = item_width(count, size);
    let height = width * CARD_HEIGHT / CARD_WIDTH;
    let gap = SPACE.gap / per_point(size);
    let row = count as f32 * width + count.saturating_sub(1) as f32 * gap;
    let right = area.x + f32::midpoint(area.width, row);
    let top = row_top(count, size);
    (0..count).map(move |at| Rect {
        x: right - width - at as f32 * (width + gap),
        y: top,
        width,
        height,
    })
}

/// Before which item of `prompt`'s row an item let go at the window's `x` lies, on a window
/// `size`, as `OrderRow`'s `before` finds it: the count of items whose middle, where it lay
/// before anything was carried, is right of `x`. 0 is the front, the first to resolve, and the
/// row's length its end.
#[must_use]
pub(crate) fn slot_at(prompt: &PromptView, size: Size, x: f32) -> Option<usize> {
    let ShapeBody::Order { items } = &prompt.body else {
        return None;
    };
    Some(slot_in(items.len(), size, x))
}

/// `slot_at` for a row of `count` items.
fn slot_in(count: usize, size: Size, x: f32) -> usize {
    (cells(count, size))
        .filter(|cell| cell.x + cell.width / 2.0 > x)
        .count()
}

/// The cell of the item `on` in `prompt`'s row on a window `size`, if the row has it.
#[must_use]
pub(crate) fn cell_of(prompt: &PromptView, size: Size, on: Subject) -> Option<Rect> {
    place_of(prompt, on).and_then(|(at, count)| cells(count, size).nth(at))
}

/// The place of the item `on` in `prompt`'s row, from the first to resolve, and how many items
/// the row has.
fn place_of(prompt: &PromptView, on: Subject) -> Option<(usize, usize)> {
    let ShapeBody::Order { items } = &prompt.body else {
        return None;
    };
    Some((items.iter().position(|item| *item == on)?, items.len()))
}

/// The item `on` of `prompt`'s row as the float that the pointer carries draws it: as the row
/// draws it, ringed in gold as `.stage-order-item.lifted` is, with no press of its own.
#[must_use]
pub(crate) fn carried_item(
    prompt: &PromptView,
    on: Subject,
    view: Option<&PlayerView>,
    faces: Option<&FaceList>,
    size: Size,
) -> Option<Element> {
    let cell = cell_of(prompt, size, on)?;
    let (place, _) = place_of(prompt, on)?;
    let mut element = item((on, place), view, faces, cell);
    element.press = None;
    Some(element.border((color::GOLD, size::ORDER_ITEM_RING)))
}

/// The marks and arrows the client draws itself for an order: none, since a waiting trigger is
/// not on the table and the session marks nothing.
pub(super) fn own(_shown: Option<Shown>, _was: Option<Shown>, _steps: &mut Steps) {}

#[cfg(test)]
mod tests {
    use super::{cells, slot_in};
    use kit_ui::input::Size;

    const SIZE: Size = Size {
        width: 1600.0,
        height: 900.0,
    };

    #[test]
    fn an_item_let_go_past_either_end_lands_at_that_end() {
        let row: Vec<_> = cells(3, SIZE).collect();
        assert!(
            row[0].x > row[1].x && row[1].x > row[2].x,
            "the first to resolve lies at the right: {row:?}"
        );
        let left = row[2].x - 1.0;
        let right = row[0].x + row[0].width + 1.0;
        assert_eq!(
            slot_in(3, SIZE, left),
            3,
            "past the left end is the last slot"
        );
        assert_eq!(
            slot_in(3, SIZE, right),
            0,
            "past the right end is the front"
        );
        let between = row[1].x + row[1].width + 1.0;
        assert_eq!(
            slot_in(3, SIZE, between),
            1,
            "between the first two is before the second"
        );
    }
}
