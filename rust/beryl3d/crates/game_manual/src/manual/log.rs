//! The manual table's log (docs/manual-multiplayer.md, "The log, undo and rewind"): a panel at
//! the table's left listing every applied action, numbered, with who did it. A line that can be
//! undone, or that a later line blocks, has an Undo button, and every line has Rewind to here,
//! which restores the table to the moment after it. A struck line is one a rewind left behind.
//! When the table refuses an undo because later lines depend on it, the log opens with those
//! lines highlighted (`sendHistory` in `ui/src/table/tableLog.ts`). The log stays folded to a
//! button until it is opened, as the web's desktop table always folds it (`TableLog` is given
//! `folded` in `ui/src/stage/Chrome.tsx`), so that it never lies over the cards. The button is
//! the web's icon button (`.game-log-toggle`): on the desktop at the game log's place in the
//! window's top left, and on a phone in the safe area's top right corner, since the desktop's
//! corner lies over the opponent's lands there. This is the web's `TableLog`
//! (`ui/src/table/TableLog.tsx`), which is the game's `LogPanel` with the table's lines.

use crate::features::log::Message as LogMessage;
use beryl_protocol::protocol::{LogLine, TableAction};
use glam::Vec2;

use super::commands;
use kit_base::tokens::{color, size, space, type_style};
use kit_base::{data::flat::ManualLog, geometry::Rect};
use kit_frame::frame::Role;
use kit_ui::element::{Align, Anchor, Element, Overflow, column, holder, row, text};
use kit_ui::widgets::{button, icon_button, icons};

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

/// The manual log either folds into one slot or opens from a separate button.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Placement {
    Folded(Rect),
    Column { toggle: Rect, panel: Rect },
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
    placement: Placement,
    metrics: ManualLog,
) -> Option<Element> {
    if lines.is_empty() {
        return None;
    }
    let name = if open { "Hide log" } else { "Show log" };
    let toggle = |button: Element| button.selected(open).on_press(LogMessage::Toggle);
    let lines = || {
        let mut list = column()
            .name("lines")
            .gap(space::S)
            .overflow(Overflow::Scroll)
            .max_height(metrics.height)
            .children(
                lines
                    .iter()
                    .map(|each| line(each, blocked.contains(&each.n))),
            );
        list.role = Role::List;
        list
    };
    let panel = |width: f32| {
        column()
            .name(TITLE)
            .width(width)
            .gap(space::S)
            .padding(space::M)
            .background(color::PANEL)
            .border((color::METAL, size::BORDER))
            .radius(size::PANEL_RADIUS)
    };
    let rect = match placement {
        Placement::Folded(rect) => rect,
        Placement::Column {
            toggle: corner,
            panel: area,
        } => {
            let mut button = toggle(icon_button(icons::LOG, name));
            button.anchor = Some(Anchor::Point(Vec2::new(corner.x, corner.y), Vec2::ZERO));
            let open = open.then(|| {
                let mut shown = panel(metrics.width).child(lines());
                shown.anchor = Some(Anchor::Point(Vec2::new(area.x, area.y), Vec2::ZERO));
                shown
            });
            return Some(holder().name("table log").child(button).children(open));
        }
    };
    let corner = Anchor::Point(Vec2::new(rect.x + rect.width, rect.y), Vec2::X);
    let heading = toggle(icon_button(icons::LOG, name));
    if !open {
        let mut folded = heading;
        folded.anchor = Some(corner);
        return Some(folded);
    }
    let mut shown = panel(rect.width).child(heading).child(lines());
    shown.anchor = Some(corner);
    Some(shown)
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
                .on_press(LogMessage::Undo(line.n))
        }))
        .child(
            button(rewind_label)
                .name(rewind_label)
                .on_press(LogMessage::Rewind(line.n)),
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
