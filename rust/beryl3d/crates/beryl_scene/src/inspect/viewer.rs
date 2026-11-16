//! The viewer, which shows the cards of a pile such as a graveyard, and `overlay`, the shell it is
//! drawn in, which every choice screen of the web interface uses (`Overlay` in
//! `ui/src/components/Overlay.tsx`): a title and an instruction over a column of content, a side
//! column for the source card and the screen's buttons, all over a darkened table.
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
use crate::element::{
    Align, Anchor, Dismiss, Element, Overflow, Position, Press, Shown, TextAlign, Track, Viewed,
    column, row, text,
};
use crate::faces::FaceList;
use crate::frame::Role;
use crate::hud::piles::pile_cards;
use crate::input::{FaceId, FaceSize, Size};
use crate::screens::ScreenIn;
use crate::table::{CARD_HEIGHT, CARD_WIDTH, pile_name};
use crate::tokens::{color, size, space, type_style};
use crate::widgets::button;

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

/// The viewer the game screen shows, if one is open (`ScreenIn::viewing`): the cards of a pile,
/// the top one first, or the faces of the Revealed Cards pile, in a grid that scrolls past two
/// rows. Its close button and Escape close it (`Dismiss::Viewer`).
#[must_use]
pub fn viewer(input: &ScreenIn) -> Option<Element> {
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
    let close = button("Close")
        .name("close")
        .on_press(Press::Dismiss(Dismiss::Viewer));
    Some(overlay(
        input.window.size,
        (&title, Some(&count_text(count, input.viewing?))),
        None,
        grid(boxes),
        vec![close],
    ))
}

/// What the viewer of `side`'s pile of `zone` is called, as the web's `pileTitle` calls it.
fn pile_title(side: Side, zone: ZoneName) -> &'static str {
    match (side, zone) {
        (Side::You, ZoneName::Library) => "Library",
        (Side::Opponent, ZoneName::Library) => "Opponent's Library",
        _ => pile_name(side, zone),
    }
}

/// How many cards the viewer lists, as the web's `viewerCount` says it, with " · top first"
/// after it for a pile of more than one card.
fn count_text(count: usize, viewed: Viewed) -> String {
    let plural = if count == 1 { "" } else { "s" };
    let library = matches!(viewed, Viewed::Pile(_, ZoneName::Library));
    let card = if library { "Card" } else { "card" };
    let top = if count > 1 && matches!(viewed, Viewed::Pile(..)) {
        " · top first"
    } else {
        ""
    };
    format!("{count} {card}{plural}{top}")
}

/// The scrolling grid of `boxes`, as many columns as the cards fill up to
/// `size::VIEWER_COLUMNS`, or "Nothing here." for none.
fn grid(boxes: Vec<Element>) -> Element {
    if boxes.is_empty() {
        return text("Nothing here.").color(color::TEXT_DIM);
    }
    #[expect(clippy::cast_precision_loss, reason = "a handful of columns")]
    let columns = (boxes.len() as f32).min(size::VIEWER_COLUMNS);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a few"
    )]
    let columns = vec![Track::Points(size::VIEWER_CARD); columns as usize];
    let mut grid = Element::new(Role::List)
        .name("cards")
        .grid_template_columns(columns)
        .gap(size::VIEWER_GAP)
        .max_height(size::VIEWER_HEIGHT)
        .overflow(Overflow::Scroll)
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
fn object_box(view: &PlayerView, slot: ObjectId) -> Element {
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

/// The web gallery's scry sample (`ScryOverlay` in `ui/src/screens/Scry.tsx`): the overlay
/// shell with two card backs and two faces fanned in its choice column and the spell that scries
/// beside it, drawn flat from `faces` where they have the cards.
#[must_use]
pub fn scry_sample(window: Size, faces: Option<&FaceList>) -> Element {
    let find = |name: &str| faces.and_then(|faces| faces.find(name));
    // Each card's centre, width and face, from `ScryOverlay`; none for a card back.
    let cards: [(Vec2, f32, Option<FaceId>, &str); 4] = [
        (Vec2::new(420.0, 174.0), 220.0, None, "back"),
        (Vec2::new(482.0, 171.0), 220.0, None, "back"),
        (
            Vec2::new(715.0, 229.0),
            273.0,
            find("Counterspell"),
            "Counterspell",
        ),
        (Vec2::new(315.0, 269.5), 297.0, find("Island"), "Island"),
    ];
    let zone = Element::new(Role::Region)
        .name("scry zone")
        .width(size::OVERLAY_MAIN)
        .height(size::VIEWER_HEIGHT)
        .children(cards.into_iter().map(|(centre, width, face, name)| {
            let height = width * CARD_HEIGHT / CARD_WIDTH;
            let mut card = Element::new(Role::Region)
                .name(name)
                .position(Position::Absolute)
                .inset([
                    Some(centre.y - height / 2.0),
                    None,
                    None,
                    Some(centre.x - width / 2.0),
                ])
                .width(width)
                .height(height)
                .background(color::SURFACE_RAISED)
                .radius(size::PANEL_RADIUS);
            card.shows = face.map(|face| Shown::Face(face, FaceSize::Board));
            card
        }));
    let mut aside = card_box("Preordain")
        .width(size::OVERLAY_SIDE)
        .height(size::OVERLAY_SIDE * CARD_HEIGHT / CARD_WIDTH);
    aside.shows = find("Preordain").map(|face| Shown::Face(face, FaceSize::Board));
    overlay(
        window,
        (
            "Scry",
            Some("Drag cards to put them on the top or bottom of your library."),
        ),
        Some(aside),
        zone,
        vec![button("Done")],
    )
}
