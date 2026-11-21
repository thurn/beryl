//! The question the Quit key asks on a manual table (docs/ui-conventions.md, "Quitting from a
//! manual table"): whether to leave the table and quit Beryl, with Quit and Cancel. The web app
//! asks it in a native dialog, in the words of `table_quit` (`rust/crates/beryl_app/src/
//! table_keys.rs`); Beryl 3D draws no native dialog, so it stands as a panel in the middle of
//! the window, which Escape or Cancel closes and Return or Quit answers.

use glam::Vec2;

use super::TablePress;
use crate::element::{Align, Anchor, Element, column, row, text};
use crate::input::Size;
use crate::tokens::{color, size, space, type_style};
use crate::widgets::{button, primary};

/// The question, in the middle of a window of `window`'s size.
#[must_use]
pub fn question(window: Size) -> Element {
    let buttons = row()
        .gap(space::M)
        .child(button("Cancel").name("close").on_press(TablePress::Close))
        .child(primary("Quit").on_press(TablePress::Quit));
    let mut panel = column()
        .name("quit")
        .gap(space::L)
        .padding(space::L)
        .align_items(Align::Center)
        .background(color::PANEL)
        .border((color::METAL, size::BORDER))
        .radius(size::PANEL_RADIUS)
        .child(text("Quit Beryl?").style(type_style::TITLE))
        .child(buttons);
    panel.anchor = Some(Anchor::Point(
        Vec2::new(window.width / 2.0, window.height / 2.0),
        Vec2::new(0.5, 0.5),
    ));
    panel
}
