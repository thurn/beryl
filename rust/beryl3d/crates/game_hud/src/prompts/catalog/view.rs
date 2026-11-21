//! The prompt shape `Catalog`: one entry of a list, such as a color or a creature type, as the
//! web interface's `Entries` (`ui/src/stage/Choose.tsx`) draws it. The table is shaded, as
//! `TrayUnder` shades it under a tray, and the entries stand in a column in the space a tray
//! would hold cards up in, each a button that sends `Click::Entry` with its index. The entry
//! the session says is chosen is lit. A list longer than its space scrolls (`widgets::list`).

use super::Catalog;
use crate::prompts::shade;

use beryl_protocol::prompt::{Click, PromptView, ShapeBody};
use glam::Vec2;

use crate::hud::HudIn;
use kit_base::tokens::{color, size, type_style};
use kit_frame::frame::Role;
use kit_ui::element::{Align, Anchor, Element, Justify, column};
use kit_ui::widgets::list;

/// What the flat layer shows for `prompt`: the shade over the table, then the prompt bar, then
/// the entries over the shade. The shade comes first so that the bar is drawn over it, and it
/// lets the pointer through, as the web's `.stage-tray` does; the table takes no press while
/// it is up (`super::covers_table`).
pub(super) fn view(prompt: &PromptView, input: &HudIn, bar: Element) -> Element {
    let mut bar = bar;
    if let ShapeBody::Catalog { entries, chosen } = &prompt.body {
        bar.children.insert(0, shade(input));
        bar.children.push(entries_column(entries, *chosen, input));
    }
    bar
}

/// The entries in the session's order, in a scrolling list as wide as its widest entry, centred
/// in the tray's space of the desktop layout as `.stage-choose` centres it, with the one `chosen`
/// lit.
fn entries_column(entries: &[String], chosen: Option<u16>, input: &HudIn) -> Element {
    // A phone gives the column the opponent's half of the table, down to the middle band, and
    // keeps every entry as tall as a finger, scrolling the list where they do not fit
    // (`.phone-board .stage-entries` in `ui/src/mobile/phone.css`).
    let slot = input.profile.slot("prompt.catalog");
    let space = slot.map_or_else(
        || input.profile.tray().on_window(input.window.size).0,
        |slot| slot.rect,
    );
    let rows = (entries.iter().enumerate()).map(|(index, words)| {
        let index = u16::try_from(index).unwrap_or(u16::MAX);
        let entry = entry(words, index, chosen == Some(index));
        if slot.is_some() {
            entry.shrink(0.0)
        } else {
            entry
        }
    });
    let entries = list("entries", rows)
        .gap(size::ENTRY_GAP)
        .align_items(Align::Stretch)
        .max_height(space.height);
    let mut placed = column()
        .name("catalog")
        .width(space.width)
        .height(space.height)
        .justify_content(Justify::Center)
        .align_items(Align::Center)
        .child(entries);
    placed.anchor = Some(Anchor::Point(Vec2::new(space.x, space.y), Vec2::ZERO));
    placed
}

/// One entry: a pill showing `words` that sends its `index`, as `.stage-entry` draws it, its
/// face gold and its rim the full gold while it is the one `chosen`.
fn entry(words: &str, index: u16, chosen: bool) -> Element {
    let mut element = Element::new(Role::Button).name(words);
    element.text = Some(words.to_owned());
    element.style.text = type_style::ENTRY;
    element.style.text_align = kit_ui::element::TextAlign::Center;
    element.style.padding = [0.0, size::ENTRY_PADDING, 0.0, size::ENTRY_PADDING];
    let (face, rim) = if chosen {
        (color::ENTRY_CHOSEN, color::GOLD)
    } else {
        (color::ENTRY, color::ENTRY_RIM)
    };
    element
        .min_width(size::ENTRY_MIN_WIDTH)
        .height(size::ENTRY_HEIGHT)
        .radius(size::ENTRY_HEIGHT / 2.0)
        .background(face)
        // A 1-point rim, as wide as a tray pile's: `.stage-entry`'s `border: 1px`.
        .border((rim, size::TRAY_PILE_BORDER))
        .color(color::TEXT)
        .selected(chosen)
        .on_press(Catalog::message(Click::Entry { index }))
}
