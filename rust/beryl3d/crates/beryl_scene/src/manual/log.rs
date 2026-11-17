//! The manual table's log (docs/manual-multiplayer.md, "The log, undo and rewind"): a panel at
//! the table's left listing every applied action, numbered, with who did it. A line that can be
//! undone, or that a later line blocks, has an Undo button, and every line has Rewind to here,
//! which restores the table to the moment after it. A struck line is one a rewind left behind.
//! When the table refuses an undo because later lines depend on it, the log opens with those
//! lines highlighted (`sendHistory` in `ui/src/table/tableLog.ts`). The log stays folded to a
//! button until it is opened, as the web's desktop table always folds it (`TableLog` is given
//! `folded` in `ui/src/stage/Chrome.tsx`), so that it never lies over the cards. On a phone the
//! folded log is the web's icon button (`.game-log-toggle`) in the safe area's top right corner,
//! since the desktop's corner lies over the opponent's lands there. This is the web's
//! `TableLog` (`ui/src/table/TableLog.tsx`).

use beryl_protocol::protocol::{LogLine, TableAction};
use glam::Vec2;

use super::TablePress;
use super::commands;
use crate::element::{Align, Anchor, Element, Overflow, Press, column, row, text};
use crate::frame::Role;
use crate::table::Window;
use crate::tokens::{color, size, space, type_style};
use crate::widgets::{button, icon_button, icons};

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

/// The log at the table's left, or on a phone (`phone` is its window) at the safe area's top
/// right: a button that opens it, and the lines of `lines` under it if `open`, those numbered
/// in `blocked` highlighted, or nothing while the log is empty. The button is named as the
/// web's is.
#[must_use]
pub fn log(
    lines: &[LogLine],
    open: bool,
    blocked: &[u32],
    phone: Option<Window>,
) -> Option<Element> {
    if lines.is_empty() {
        return None;
    }
    let name = if open { "Hide log" } else { "Show log" };
    let (heading, corner, metrics) = if let Some(window) = phone {
        use crate::data::space::flat::phone::GUTTER;
        let safe = window.safe_area;
        let right = window.size.width - safe.right - GUTTER;
        let corner = Anchor::Point(Vec2::new(right, safe.top + GUTTER), Vec2::X);
        let width = crate::data::space::flat::table::log::WIDTH
            .min(window.size.width - safe.left - safe.right - 2.0 * GUTTER);
        (icon_button(icons::LOG, name), corner, width)
    } else {
        let left = crate::data::space::flat::table::EDGE;
        let top = crate::data::space::flat::table::log::TOP;
        let corner = Anchor::Point(Vec2::new(left, top), Vec2::ZERO);
        let width = crate::data::space::flat::table::log::WIDTH;
        (button(TITLE).name(name), corner, width)
    };
    let heading = heading
        .selected(open)
        .on_press(Press::Table(TablePress::ToggleLog));
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
        .children(
            lines
                .iter()
                .map(|each| line(each, blocked.contains(&each.n))),
        );
    list.role = Role::List;
    panel = panel.child(list);
    panel.anchor = Some(corner);
    Some(panel)
}

/// One line of the log: its number, who, what, and its controls, highlighted as the web's
/// `[data-blocks-undo]` line is (`ui/src/screens/styles/table.css`) if it `blocks` an undo.
fn line(line: &LogLine, blocks: bool) -> Element {
    let words = format!("{} {} {}", line.n, line.by, line.text);
    let ink = if blocks {
        color::TEXT
    } else if line.struck {
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
    let shown = column()
        .name(&format!("line {}", line.n))
        .gap(space::S)
        .align_items(Align::Start)
        .selected(blocks)
        .child(text(&words).style(type_style::BODY).color(ink))
        .child(controls);
    if blocks {
        shown.background(BLOCKS_UNDO).radius(size::MENU_ROW_RADIUS)
    } else {
        shown
    }
}

/// The background of a line that blocks an undo: the web's `rgb(214 192 150 / 22%)`.
const BLOCKS_UNDO: [u8; 4] = [214, 192, 150, 56];
