//! The viewer that shows cards from a pile, such as a graveyard, or the Revealed Cards pile.
//!
//! A pile's cards are its scene objects (`Shown::Object`): each box of the viewer's grid shows
//! one, and the table moves the card from its pile into the box and back (`table::shown`), so
//! that it is the same card on the table and in the viewer. The faces of the Revealed Cards
//! pile are second elements, not scene objects, so its viewer draws them flat (`Shown::Face`).

use beryl_protocol::PlayerView;
use beryl_protocol::protocol::{ObjectId, Side, ZoneName};
use beryl_protocol::script::Subject;
use glam::Vec2;

use super::revealed::{REVEALED, RevealedFace};
use crate::data::space::flat::gallery as metrics;
use crate::element::{
    Align, Anchor, Dismiss, Element, Overflow, Press, Shown, TextAlign, Track, Viewed, column, row,
    text,
};
use crate::faces::{CARD_BACK, FaceList};
use crate::frame::Role;
use crate::hud::HudIn;
use crate::hud::piles::pile_cards;
use crate::input::{FaceId, FaceSize, Size};
use crate::table::{CARD_HEIGHT, CARD_WIDTH, pile_name};
use crate::tokens::{color, size, space, type_style};
use crate::widgets::{icon_button, icons};

/// The viewer the game screen shows, if one is open (`HudIn::viewing`): the cards of a pile,
/// the top one first, or the faces of the Revealed Cards pile, in a grid that scrolls past two
/// rows. Its close button and Escape close it (`Dismiss::Viewer`).
#[must_use]
pub fn viewer(input: &HudIn) -> Option<Element> {
    let (title, boxes) = match input.viewing? {
        Viewed::Pile(side, zone) => {
            let view = input.view?;
            let mut slots = pile_cards(view, side, zone);
            slots.reverse();
            let boxes: Vec<Element> = slots.iter().map(|&slot| object_box(view, slot)).collect();
            (pile_title(side, zone).to_owned(), boxes)
        }
        Viewed::Revealed => {
            let boxes = input.revealed.iter().map(face_box).collect();
            (REVEALED.to_owned(), boxes)
        }
    };
    let count = boxes.len();
    let close = icon_button(icons::CLOSE, "close").on_press(Press::Dismiss(Dismiss::Viewer));
    Some(viewer_dialog(
        input.window.size,
        (&title, &count_text(count, input.viewing?)),
        count,
        size::VIEWER_MIN_WIDTH,
        Some(close),
        grid(boxes),
        vec![],
    ))
}

/// A pile viewer uses the web's centered `.zone-viewer` dialog, whose title shares a row
/// with its count and close control. The full choice overlay puts a large title at the
/// opponent's life total instead. Cards keep their existing grid and travel into its boxes.
pub(crate) fn viewer_dialog(
    window: Size,
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
    panel.anchor = Some(Anchor::Point(
        Vec2::new(window.width, window.height) / 2.0,
        Vec2::splat(0.5),
    ));
    let mut shell = column()
        .name("overlay")
        .width(window.width)
        .height(window.height)
        .background(color::BACKDROP)
        .child(panel);
    shell.anchor = Some(Anchor::Point(Vec2::ZERO, Vec2::ZERO));
    shell
}

/// What the viewer of `side`'s pile of `zone` is called, as the web's `pileTitle` calls it.
pub(crate) fn pile_title(side: Side, zone: ZoneName) -> &'static str {
    match (side, zone) {
        (Side::You, ZoneName::Library) => "Library",
        (Side::Opponent, ZoneName::Library) => "Opponent's Library",
        _ => pile_name(side, zone),
    }
}

/// How many cards the viewer lists, as the web's `viewerCount` says it, with " · top first"
/// after it for a pile of more than one card.
fn count_text(count: usize, viewed: Viewed) -> String {
    let library = matches!(viewed, Viewed::Pile(_, ZoneName::Library));
    count_words(count, library, matches!(viewed, Viewed::Pile(..)))
}

/// `count` cards, "Card" in a library's viewer and "card" elsewhere, with " · top first" after
/// it for more than one card listed `top_first`.
pub(crate) fn count_words(count: usize, library: bool, top_first: bool) -> String {
    let plural = if count == 1 { "" } else { "s" };
    let card = if library { "Card" } else { "card" };
    let top = if count > 1 && top_first {
        " · top first"
    } else {
        ""
    };
    format!("{count} {card}{plural}{top}")
}

/// The scrolling grid of `boxes`, as many columns as the cards fill up to
/// `size::VIEWER_COLUMNS`, or "Nothing here." for none.
pub(crate) fn grid(boxes: Vec<Element>) -> Element {
    if boxes.is_empty() {
        return text("Nothing here.").color(color::TEXT_DIM);
    }
    cards(boxes)
        .name("cards")
        .max_height(size::VIEWER_HEIGHT)
        .overflow(Overflow::Scroll)
        .scroll_fade(size::VIEWER_FADE)
}

/// The grid of `boxes`, as many columns as the cards fill up to `size::VIEWER_COLUMNS`.
pub(crate) fn cards(boxes: Vec<Element>) -> Element {
    #[expect(clippy::cast_precision_loss, reason = "a handful of columns")]
    let columns = (boxes.len() as f32).min(size::VIEWER_COLUMNS);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a few"
    )]
    let columns = vec![Track::Points(size::VIEWER_CARD); columns as usize];
    let mut grid = Element::new(Role::List)
        .grid_template_columns(columns)
        .gap(size::VIEWER_GAP)
        .children(boxes);
    grid.style.display = crate::element::Display::Grid;
    grid
}

/// A box of the grid, a card's size, named `name`.
fn card_box(name: &str) -> Element {
    Element::new(Role::Region)
        .name(name)
        .width(size::VIEWER_CARD)
        .height(size::VIEWER_CARD * CARD_HEIGHT / CARD_WIDTH)
}

/// The box that shows the scene object in `slot`, named for its card and slot, as a locator
/// names a card ("Island #8").
pub(crate) fn object_box(view: &PlayerView, slot: ObjectId) -> Element {
    let object = view.objects.iter().find(|object| object.slot == slot);
    let name = object
        .and_then(|object| object.name.as_deref())
        .unwrap_or("card");
    let mut element = card_box(&format!("{name} #{}", slot.0));
    element.shows = Some(Shown::Object(Subject::Object { slot }));
    element
}

/// The box that shows a revealed face flat.
fn face_box(face: &RevealedFace) -> Element {
    let mut element = card_box(&format!("{} #{}", face.name, face.slot.0));
    element.shows = face.face.map(|id| Shown::Face(id, FaceSize::Board));
    element
}

/// The web gallery's Scry sample, as an embedded preview of the overlay's zones,
/// source card and action.
#[must_use]
pub fn scry_sample(faces: &FaceList) -> Element {
    let find = |name| faces.find(name).expect("the gallery lists the scry face");
    let card = |id: FaceId, name, width| {
        let mut card = card_box(name)
            .width(width)
            .height(width * CARD_HEIGHT / CARD_WIDTH)
            .background(color::SURFACE);
        card.shows = Some(Shown::Face(id, FaceSize::Board));
        card
    };
    let zone = |label: &str, cards: Vec<Element>| {
        column()
            .align_items(Align::Center)
            .gap(space::S)
            .child(row().gap(space::S).children(cards))
            .child(text(label).style(type_style::OVERLAY_INSTRUCTION))
    };
    let bottom = zone(
        "Bottom",
        vec![
            card(CARD_BACK, "First card back", metrics::SCRY_CARD),
            card(CARD_BACK, "Second card back", metrics::SCRY_CARD),
        ],
    );
    let top = zone(
        "Top",
        vec![
            card(find("Island"), "Island", metrics::SCRY_CARD),
            card(find("Quell"), "Quell", metrics::SCRY_CARD),
        ],
    );
    let choice = column()
        .name("choice")
        .align_items(Align::Center)
        .gap(space::M)
        .child(text("Scry").style(type_style::OVERLAY_TITLE))
        .child(
            text("Drag cards to put them on the top or bottom of your library.")
                .style(type_style::OVERLAY_INSTRUCTION)
                .text_align(TextAlign::Center),
        )
        .child(row().gap(space::M).wrap(true).children([bottom, top]))
        .child(crate::widgets::primary("Done"));
    row()
        .name("overlay")
        .padding(space::L)
        .gap(space::L)
        .wrap(true)
        .align_items(Align::Center)
        .justify_content(crate::element::Justify::Center)
        .background(color::BACKDROP)
        .child(choice)
        .child(card(find("Foresee"), "Foresee", metrics::SCRY_SOURCE))
}
