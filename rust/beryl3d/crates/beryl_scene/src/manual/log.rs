//! The manual table's log (docs/manual-multiplayer.md, "The log, undo and rewind"): a panel at
//! the table's left listing every applied action, numbered, with who did it. A line that can be
//! undone, or that a later line blocks, has an Undo button, and every line has Rewind to here,
//! which restores the table to the moment after it. A struck line is one a rewind left behind.
//! The log stays folded to a button until it is opened, as the web's log folds while cards
//! reach into its column (`GameLog` in `ui/src/game/Narration.tsx`), so that it never lies over
//! the cards. This is the web's `TableLog` (`ui/src/table/TableLog.tsx`).

use beryl_protocol::protocol::{LogLine, TableAction};
use glam::Vec2;

use super::TablePress;
use super::commands;
use crate::element::{Align, Anchor, Element, Overflow, Press, column, row, text};
use crate::frame::Role;
use crate::tokens::{color, size, space, type_style};
use crate::widgets::button;

/// The panel's name and its heading's words.
pub const TITLE: &str = "Log";

/// The action a line's Undo sends.
#[must_use]
pub fn undo(entry: u32) -> TableAction {
    serde_json::from_value(serde_json::json!({ "undo": { "entry": entry } }))
        .expect("an undo is a table action")
}

/// The action a line's Rewind to here sends.
#[must_use]
pub fn rewind(to: u32) -> TableAction {
    serde_json::from_value(serde_json::json!({ "rewind": { "to": to } }))
        .expect("a rewind is a table action")
}

/// The log at the table's left: a button that opens it, and the lines of `lines` under it if
/// `open`, or nothing while the log is empty. The button is named as the web's is.
#[must_use]
pub fn log(lines: &[LogLine], open: bool) -> Option<Element> {
    if lines.is_empty() {
        return None;
    }
    let metrics = crate::data::space::flat::table::log::WIDTH;
    let heading = button(TITLE)
        .name(if open { "Hide log" } else { "Show log" })
        .selected(open)
        .on_press(Press::Table(TablePress::ToggleLog));
    let left = crate::data::space::flat::table::EDGE;
    let top = crate::data::space::flat::table::log::TOP;
    let corner = Anchor::Point(Vec2::new(left, top), Vec2::ZERO);
    if !open {
        let mut folded = heading;
        folded.anchor = Some(corner);
        return Some(folded);
    }
    let mut panel = column()
        .name(TITLE)
        .width(metrics)
        .gap(space::S)
        .padding(space::M)
        .background(color::PANEL)
        .border((color::METAL, size::BORDER))
        .radius(size::PANEL_RADIUS)
        .child(heading);
    let mut list = column()
        .name("lines")
        .gap(space::S)
        .overflow(Overflow::Scroll)
        .max_height(crate::data::space::flat::table::log::HEIGHT)
        .children(lines.iter().map(line));
    list.role = Role::List;
    panel = panel.child(list);
    panel.anchor = Some(corner);
    Some(panel)
}

/// One line of the log: its number, who, what, and its controls.
fn line(line: &LogLine) -> Element {
    let words = format!("{} {} {}", line.n, line.by, line.text);
    let ink = if line.struck {
        color::TEXT_FAINT
    } else if line.by == "You" {
        color::TEXT
    } else {
        color::TEXT_DIM
    };
    let undoable = line.undoable || !line.blocked_by.is_empty();
    let rewind_label = commands::command("table.rewind").map_or("Rewind to here", |(_, c)| c.label);
    let controls = row()
        .name("controls")
        .gap(space::S)
        .wrap(true)
        .children(undoable.then(|| {
            button("Undo")
                .name("Undo")
                .on_press(Press::Table(TablePress::Undo(line.n)))
        }))
        .child(
            button(rewind_label)
                .name(rewind_label)
                .on_press(Press::Table(TablePress::Rewind(line.n))),
        );
    column()
        .name(&format!("line {}", line.n))
        .gap(space::S)
        .align_items(Align::Start)
        .child(text(&words).style(type_style::BODY).color(ink))
        .child(controls)
}
