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

use super::Order;

use beryl_protocol::prompt::{Click, PromptView, ShapeBody};
use beryl_protocol::protocol::{Candidate, CandidateView, PlayerView, Status};
use beryl_protocol::script::Subject;
use glam::Vec2;

use crate::hud::HudIn;
use crate::prompts::feature::Details;
use game_table::faces::FaceList;
use game_table::profiles::Profile;
use game_table::table::{TraySpace, tray_area};
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

/// What the flat layer shows for `prompt`: the shade over the table, the waiting triggers in
/// their row with the session's words for its two ends under it, and the prompt bar. The item
/// `carried`, which the pointer carries along the row, is left out of it: a float draws it
/// under the pointer (`carried_item`).
pub(super) fn view(prompt: &PromptView, input: &HudIn, bar: Element) -> Element {
    let ShapeBody::Order { items } = &prompt.body else {
        return bar;
    };
    if items.is_empty() {
        return bar;
    }
    let size = input.window.size;
    let tray = (input.profile.order_tray(), size);
    let mut shade = row()
        .name("shade")
        .width(size.width)
        .height(size.height)
        .background(color::TRAY_SHADE)
        .passes(true);
    shade.anchor = Some(Anchor::Point(Vec2::ZERO, Vec2::ZERO));
    let row_items = (items.iter().zip(cells(items.len(), tray)).enumerate())
        .filter(|(_, (on, _))| input.carried != Some(**on))
        .map(|(place, (&on, cell))| {
            let mut element = item((on, place), input.view, input.faces, cell);
            element.anchor = Some(Anchor::Point(Vec2::new(cell.x, cell.y), Vec2::ZERO));
            element
        });
    row()
        .name("order")
        .child(shade)
        .children(row_items)
        .child(ends(prompt, items.len(), tray))
        .child(bar)
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
    let (name, face) = item_face(on, view, faces);
    Element {
        shows: face.map(|face| Showing::Face(face, FaceSize::Board)),
        ..Element::new(Role::Ability)
    }
    .name(&format!("{}. {}", place + 1, name))
    .width(cell.width)
    .height(cell.height)
    .radius(size::ORDER_ITEM_RADIUS)
    .on_press(Order::message(Click::Drop { on, pile: 0, at: 0 }))
}

fn item_face<'a>(
    on: Subject,
    view: Option<&'a PlayerView>,
    faces: Option<&FaceList>,
) -> (&'a str, Option<kit_ui::input::FaceId>) {
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
    (name.unwrap_or(""), face)
}

pub(super) fn details(input: &HudIn<'_>) -> Details {
    let items = match input.prompt.map(|prompt| &prompt.body) {
        Some(ShapeBody::Order { items }) => items.as_slice(),
        _ => &[],
    };
    let items = items
        .iter()
        .map(|&on| {
            let (name, face) = item_face(on, input.view, input.faces);
            (name.to_owned(), face)
        })
        .collect();
    Details::Order {
        items,
        carried: input.carried,
    }
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
fn ends(prompt: &PromptView, count: usize, tray: (TraySpace, Size)) -> Element {
    let (space, size) = tray;
    let [last, first] = &prompt.words.piles;
    let (per_point, area) = (per_point(tray), tray_area(space, size));
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
    let row_bottom =
        row_top(count, tray) + item_width(count, tray) * tray.0.card.height / tray.0.card.width;
    let top = row_bottom + ENDS_GAP / per_point;
    ends.anchor = Some(Anchor::Point(
        Vec2::new(area.x + (area.width - width) / 2.0, top),
        Vec2::ZERO,
    ));
    ends
}

/// Logical points to stage pixels on a window `size`.
fn per_point((space, size): (TraySpace, Size)) -> f32 {
    space.stage_height / size.height
}

/// How wide each of `count` items is drawn, in logical points, as `OrderRow` sizes them: the
/// space's width shared out with a gap beside each, kept between the least and the most a
/// card in a tray is drawn.
#[expect(clippy::cast_precision_loss, reason = "a row holds a few items")]
fn item_width(count: usize, tray: (TraySpace, Size)) -> f32 {
    let (space, size) = tray;
    let per_point = per_point(tray);
    let wide = tray_area(space, size).width * per_point;
    let gap = space.gap;
    let shared = (wide - gap) / count.max(1) as f32 - gap;
    space.least.max(space.most.min(shared)) / per_point
}

/// The top of the row and the words under it, centred together in the space, in logical
/// points.
fn row_top(count: usize, tray: (TraySpace, Size)) -> f32 {
    let (space, size) = tray;
    let per_point = per_point(tray);
    let area = tray_area(space, size);
    let tall = item_width(count, tray) * space.card.height / space.card.width
        + (ENDS_GAP + ENDS_SIZE) / per_point;
    area.y + (area.height - tall) / 2.0
}

/// Where each of `count` items lies in the row on a window `size`, in logical points, in the
/// order the prompt lists them: the first to resolve at the right and each after it to its
/// left, the row centred in the space, as `.stage-order-row`'s `row-reverse` lays them out.
#[expect(clippy::cast_precision_loss, reason = "a row holds a few items")]
fn cells(count: usize, tray: (TraySpace, Size)) -> impl Iterator<Item = Rect> {
    let (space, size) = tray;
    let area = tray_area(space, size);
    let width = item_width(count, tray);
    let height = width * space.card.height / space.card.width;
    let gap = space.gap / per_point(tray);
    let row = count as f32 * width + count.saturating_sub(1) as f32 * gap;
    let right = area.x + f32::midpoint(area.width, row);
    let top = row_top(count, tray);
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
pub fn slot_at(prompt: &PromptView, profile: Profile<'_>, x: f32) -> Option<usize> {
    let ShapeBody::Order { items } = &prompt.body else {
        return None;
    };
    Some(slot_in(
        items.len(),
        (profile.order_tray(), profile.window().size),
        x,
    ))
}

/// `slot_at` for a row of `count` items.
fn slot_in(count: usize, tray: (TraySpace, Size), x: f32) -> usize {
    cells(count, tray)
        .filter(|cell| cell.x + cell.width / 2.0 > x)
        .count()
}

/// The cell of the item `on` in `prompt`'s row on a window `size`, if the row has it.
#[must_use]
pub fn cell_of(prompt: &PromptView, profile: Profile<'_>, on: Subject) -> Option<Rect> {
    place_of(prompt, on)
        .and_then(|(at, count)| cells(count, (profile.order_tray(), profile.window().size)).nth(at))
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
pub fn carried_item(
    prompt: &PromptView,
    on: Subject,
    view: Option<&PlayerView>,
    faces: Option<&FaceList>,
    profile: Profile<'_>,
) -> Option<Element> {
    let cell = cell_of(prompt, profile, on)?;
    let (place, _) = place_of(prompt, on)?;
    let mut element = item((on, place), view, faces, cell);
    element.press = None;
    Some(element.border((color::GOLD, size::ORDER_ITEM_RING)))
}

#[cfg(test)]
mod tests {
    use super::{Profile, cells, slot_in};
    use crate::test_profiles::SHIPPED;
    use kit_base::data::Card;
    use kit_base::data::profiles::Profiles;

    #[test]
    fn an_item_let_go_past_either_end_lands_at_that_end() {
        let card = include_str!("../../../../../../../data/beryl3d/card.json");
        let profiles = Profiles::parse(&SHIPPED, Card::parse(card).unwrap()).unwrap();
        let window = profiles.fallback().usual_window();
        let profile = Profile::for_window(&profiles, window);
        let tray = (profile.order_tray(), window.size);
        let row: Vec<_> = cells(3, tray).collect();
        assert!(
            row[0].x > row[1].x && row[1].x > row[2].x,
            "the first to resolve lies at the right: {row:?}"
        );
        let left = row[2].x - 1.0;
        let right = row[0].x + row[0].width + 1.0;
        assert_eq!(
            slot_in(3, tray, left),
            3,
            "past the left end is the last slot"
        );
        assert_eq!(
            slot_in(3, tray, right),
            0,
            "past the right end is the front"
        );
        let between = row[1].x + row[1].width + 1.0;
        assert_eq!(
            slot_in(3, tray, between),
            1,
            "between the first two is before the second"
        );
    }
}
