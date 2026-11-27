//! The viewer that shows cards from a pile, such as a graveyard, or the Revealed Cards pile.
//!
//! A pile's cards are its scene objects (`Show`): each box of the viewer's grid shows
//! one, and the table moves the card from its pile into the box and back (`table::shown`), so
//! that it is the same card on the table and in the viewer. The faces of the Revealed Cards
//! pile are second elements, not scene objects, so its viewer draws them flat (`Shown::Face`).

use super::Message;
use crate::inspect::Viewed;
use beryl_protocol::PlayerView;
use beryl_protocol::protocol::{ObjectId, Side, ZoneName};
use glam::Vec2;

use super::revealed::{REVEALED, RevealedFace};
use crate::hud::piles::pile_cards;
use game_table::table::pile_name;
use kit_base::data::Card;
use kit_base::tokens::{color, size, space, type_style};
use kit_frame::frame::{ElementId, Role};
use kit_scene::{Show, grid::grid_element};
use kit_space::arrange::Grid;
use kit_ui::element::{Align, Anchor, Element, Overflow, Shown, column, row, text};
use kit_ui::input::{FaceSize, Size};
use kit_ui::widgets::{icon_button, icons};

/// The viewer the game screen shows, if one is open (`HudIn::viewing`): the cards of a pile,
/// the top one first, or the faces of the Revealed Cards pile, in a grid that scrolls past two
/// rows. Its close button and Escape close it (`Dismiss::Viewer`).
#[must_use]
pub(super) fn dialog(
    (window, centre, card): (Size, Vec2, Card),
    view: Option<&PlayerView>,
    viewing: Option<Viewed>,
    revealed: &[RevealedFace],
) -> Option<Element> {
    let (title, boxes) = match viewing? {
        Viewed::Pile(side, zone) => {
            let view = view?;
            let mut slots = pile_cards(view, side, zone);
            slots.reverse();
            let boxes: Vec<Element> = slots
                .iter()
                .map(|&slot| object_box(view, slot, card))
                .collect();
            (pile_title(side, zone).to_owned(), boxes)
        }
        Viewed::Revealed => {
            let boxes = revealed.iter().map(|face| face_box(face, card)).collect();
            (REVEALED.to_owned(), boxes)
        }
    };
    let count = boxes.len();
    let close = icon_button(icons::CLOSE, "close").on_press(Message::Close);
    let available = window.width - 2.0 * space::L;
    let columns =
        Grid::fitting_columns(available, size::VIEWER_CARD, size::VIEWER_GAP).min(viewer_columns());
    Some(viewer_dialog(
        (window, centre),
        (&title, &count_text(count, viewing?)),
        count.min(columns),
        size::VIEWER_MIN_WIDTH.min(window.width),
        Some(close),
        scroll_grid(boxes, columns),
        vec![],
    ))
}

/// A pile viewer uses the web's centered `.zone-viewer` dialog, whose title shares a row
/// with its count and close control. The full choice overlay puts a large title at the
/// opponent's life total instead. Cards keep their existing grid and travel into its boxes.
pub fn viewer_dialog(
    (window, centre): (Size, Vec2),
    (title, count): (&str, &str),
    cards: usize,
    minimum_width: f32,
    close: Option<Element>,
    body: Element,
    actions: Vec<Element>,
) -> Element {
    #[expect(clippy::cast_precision_loss, reason = "at most five columns")]
    let columns = (cards as f32).clamp(1.0, size::VIEWER_COLUMNS);
    let width = (columns * size::VIEWER_CARD + (columns - 1.0) * size::VIEWER_GAP + 2.0 * space::L)
        .max(minimum_width);
    let header = row()
        .name("header")
        .align_items(Align::Center)
        .gap(size::VIEWER_GAP)
        .child(text(title).style(type_style::TITLE).color(color::GOLD))
        .child(text(count).grow(1.0).color(color::TEXT_DIM))
        .children(close);
    let mut panel = column()
        .name("viewer")
        .width(width)
        .gap(space::M)
        .background(color::PANEL)
        .radius(space::M)
        .child(header)
        .child(body);
    if !actions.is_empty() {
        panel = panel.child(row().name("actions").gap(space::M).children(actions));
    }
    panel.style.padding = [space::M, space::L, space::L, space::L];
    panel.anchor = Some(Anchor::Point(centre, Vec2::splat(0.5)));
    let mut shell = column()
        .name("overlay")
        .width(window.width)
        .height(window.height)
        .background(color::BACKDROP)
        .holds_focus()
        .child(panel);
    shell.anchor = Some(Anchor::Point(Vec2::ZERO, Vec2::ZERO));
    shell
}

/// What the viewer of `side`'s pile of `zone` (or the shared pile when `side` is `None`) is
/// called. Shared piles use MTG Arena's titles ("Library", "Graveyard", "Exile").
pub fn pile_title(side: Option<Side>, zone: ZoneName) -> &'static str {
    match (side, zone) {
        (Some(Side::You), ZoneName::Library) => "Library",
        (Some(Side::Opponent), ZoneName::Library) => "Opponent's Library",
        _ => pile_name(side, zone),
    }
}

/// How many cards the viewer lists, as the web's `viewerCount` says it.
fn count_text(count: usize, viewed: Viewed) -> String {
    let library = matches!(viewed, Viewed::Pile(_, ZoneName::Library));
    count_words(count, library)
}

/// `count` cards, "Card" in a library's viewer and "card" elsewhere.
pub fn count_words(count: usize, library: bool) -> String {
    let plural = if count == 1 { "" } else { "s" };
    let card = if library { "Card" } else { "card" };
    format!("{count} {card}{plural}")
}

/// The scrolling grid of `boxes`, as many columns as the cards fill up to
/// `size::VIEWER_COLUMNS`, or "Nothing here." for none.
pub fn grid(boxes: Vec<Element>) -> Element {
    scroll_grid(boxes, viewer_columns())
}

fn scroll_grid(boxes: Vec<Element>, columns: usize) -> Element {
    if boxes.is_empty() {
        return text("Nothing here.").color(color::TEXT_DIM);
    }
    show_grid(Element::new(Role::List), boxes, columns)
        .name("cards")
        .max_height(size::VIEWER_HEIGHT)
        .overflow(Overflow::Scroll)
        .scroll_fade(size::VIEWER_FADE)
}

/// The grid of `boxes`, as many columns as the cards fill up to `size::VIEWER_COLUMNS`.
pub fn cards(boxes: Vec<Element>) -> Element {
    show_grid(Element::new(Role::List), boxes, viewer_columns())
}

/// Places standard-size card boxes in the grid used by a viewer or a manual tray.
pub fn show_grid(element: Element, boxes: Vec<Element>, columns: usize) -> Element {
    let count = boxes.len();
    let height = boxes
        .first()
        .and_then(|box_| box_.style.height)
        .unwrap_or(0.0);
    let size = Vec2::new(size::VIEWER_CARD, height);
    let grid = Grid::new(columns, size, Vec2::splat(size::VIEWER_GAP));
    grid_element(element, boxes, grid, grid.size(count).x, false)
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the token holds a column count"
)]
fn viewer_columns() -> usize {
    size::VIEWER_COLUMNS as usize
}

/// A box of the grid, the shape of `card`, named `name`.
fn card_box(name: &str, card: Card) -> Element {
    Element::new(Role::Region)
        .name(name)
        .width(size::VIEWER_CARD)
        .height(size::VIEWER_CARD * card.height / card.width)
}

/// The box that shows the scene object in `slot`, named for its card and slot, as a locator
/// names a card ("Island #8").
pub fn object_box(view: &PlayerView, slot: ObjectId, card: Card) -> Element {
    let object = view.objects.iter().find(|object| object.slot == slot);
    let name = object
        .and_then(|object| object.name.as_deref())
        .unwrap_or("card");
    let mut element = card_box(&format!("{name} #{}", slot.0), card);
    element.show = Some(Show::object(ElementId(u32::from(slot.0))));
    element
}

/// The box that shows a revealed face flat.
fn face_box(face: &RevealedFace, card: Card) -> Element {
    let mut element = card_box(&format!("{} #{}", face.name, face.slot.0), card);
    element.shows = face.face.map(|id| Shown::Face(id, FaceSize::Board));
    element
}
