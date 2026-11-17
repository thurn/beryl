//! The manual table's screen (docs/manual-multiplayer.md): the table is drawn as in a game,
//! with the life totals and piles of the hud, and the flat layer holds the table's keys, the
//! log at its left, and whatever the table shows over itself: the context menu a right click or
//! a long press opens, which on a phone is a bottom sheet, the command list, the ask prompt in
//! the dock, the token search or the library order tray; the viewer of a pile; the rectangle a
//! sweep draws; and the outline of where a held card would land. Only the solo table is built:
//! the web interface's host and join screens come later.

use beryl_protocol::protocol::Facing;
use glam::Vec2;

use super::ScreenIn;
use crate::data::space::flat::{MENU_EDGE, sheet};
use crate::data::space::pointer::DRAG_THRESHOLD_POINTS;
use crate::element::{Align, Element, Overflow, Press, Shown, column};
use crate::frame::Role;
use crate::input::{FaceId, FaceSize};
use crate::inspect::viewer::viewer;
use crate::manual::Target;
use crate::manual::commands::{COMMANDS, Scope};
use crate::manual::menus::{MENU, OpenMenu};
use crate::manual::{TableState, ask, drop, log, order, palette, selection, tokens};
use crate::table::{CARD_HEIGHT, CARD_WIDTH};
use crate::widgets::{context_menu, menu, sheet_menu};

/// What the flat layer shows on the table: a region with no box of its own, which answers every
/// key of a table command with `Press::Command`, the log, and the open context menu, command
/// list, ask prompt, token search, custom token form or order tray, if any.
///
/// A command's keys are the one the player bound to it, or else its own and its aliases from
/// `data/table-commands.json` (`Bindings::keys`). Those are written as the Mac's bindings:
/// `Chord::parse` reads `Cmd` as Control and `Option` as Alt elsewhere, and the file's other
/// bindings differ from the Mac's only in writing those.
#[must_use]
pub fn table(input: &ScreenIn) -> Element {
    // The region has no box of its own, since everything in it stands at a point of the
    // window, so the pointer reaches the table's cards around what it shows. It does not pass
    // presses through, which every element in it would inherit.
    let mut screen = column().name("table");
    for (index, command) in COMMANDS.iter().enumerate() {
        let Ok(index) = u16::try_from(index) else {
            break;
        };
        if command.scope != Scope::Table {
            continue;
        }
        for key in input.settings.bindings.keys(command) {
            screen = screen.shortcut(key, Press::Command(index, false));
        }
    }
    // With Shift held and no binding of its own, a token command's key makes its token face
    // down (`listenTableKeys` in `ui/src/table/keyboard.ts`). The unshifted bindings come
    // first, so a command bound to the shifted chord keeps it.
    for (index, command) in COMMANDS.iter().enumerate() {
        let Ok(index) = u16::try_from(index) else {
            break;
        };
        if command.scope != Scope::Table || !command.id.starts_with("token.") {
            continue;
        }
        for key in input.settings.bindings.keys(command) {
            if !key.split('+').any(|part| part == "Shift") {
                screen = screen.shortcut(&format!("Shift+{key}"), Press::Command(index, true));
            }
        }
    }
    let state = input.table;
    let lines = input
        .view
        .and_then(|view| view.table.as_ref())
        .map(|table| &table.log[..]);
    let log = lines.and_then(|lines| {
        log::log(
            lines,
            state.log_open,
            &state.blocked,
            input.window.phone().then_some(input.window),
        )
    });
    let open = state.menu.as_ref().map(|open| {
        if input.window.phone() {
            bottom_sheet(open, input)
        } else {
            context_menu(open.at, menu(MENU, &open.title, &open.groups))
        }
    });
    let sweep = (state.sweep)
        .filter(|(from, to)| from.distance(*to) >= DRAG_THRESHOLD_POINTS)
        .map(|(from, to)| selection::rectangle(selection::sweep_box(from, to)));
    let landing = state.landing.as_ref().map(drop::target);
    screen
        .children(log)
        .children(sweep)
        .children(landing)
        .children(viewer(input))
        .children(open)
        .children(over(state, input))
}

/// The command list, ask prompt, token search, custom token form or order tray the table
/// shows, if one is open.
fn over(state: &TableState, input: &ScreenIn) -> Option<Element> {
    let window = input.window.size;
    if let Some(open) = &state.palette {
        return Some(palette::panel(open, input.fields, window));
    }
    if let Some(open) = &state.ask {
        return Some(ask::dock(open, input.fields, window));
    }
    if let Some(open) = &state.custom {
        return Some(tokens::custom_panel(open, input.fields, window));
    }
    let view = input.view?;
    if let Some(open) = &state.tokens {
        return Some(tokens::panel(open, view, input.fields, window));
    }
    (state.order.as_ref()).map(|open| order::tray(open, view, window))
}

/// The context menu on a phone, as the web's `[data-presentation="sheet"]` menu stands: along
/// the bottom of the window above its safe area, as wide as the window up to `sheet.max_width`,
/// scrolling where it is taller than the window, with a card's full face above its rows.
fn bottom_sheet(open: &OpenMenu, input: &ScreenIn) -> Element {
    let window = input.window;
    let safe = window.safe_area;
    let width =
        (window.size.width - safe.left - safe.right - 2.0 * MENU_EDGE).min(sheet::MAX_WIDTH);
    let mut panel = sheet_menu(MENU, &open.title, &open.groups, open.page)
        .min_width(width)
        .max_width(width)
        .max_height(window.size.height - safe.top - safe.bottom - 2.0 * MENU_EDGE)
        .overflow(Overflow::Scroll);
    if let Some(face) = preview(open.target, input) {
        panel.children.insert(0, face);
    }
    // A menu whose bottom would pass the point stands with its bottom edge on it
    // (`Anchor::Menu`), so the sheet opens from the bottom, centred across the window.
    let left = safe.left + (window.size.width - safe.left - safe.right - width) / 2.0;
    context_menu(Vec2::new(left, window.size.height - safe.bottom), panel)
}

/// The full face of the card the sheet is for, `sheet.preview_width` points wide and centred,
/// as the web's `.table-menu-preview` shows it; none for a pile, a hand, a life total, the
/// table, or a card whose face the viewer cannot see.
fn preview(target: Target, input: &ScreenIn) -> Option<Element> {
    let face = preview_face(target, input)?;
    let card = Element {
        shows: Some(Shown::Face(face, FaceSize::Full)),
        ..Element::new(Role::Region)
    }
    .name("preview")
    .width(sheet::PREVIEW_WIDTH)
    .height(sheet::PREVIEW_WIDTH * CARD_HEIGHT / CARD_WIDTH);
    Some(column().align_items(Align::Center).child(card))
}

/// The face the phone's bottom sheet shows at full size, which the client asks for at that
/// size (`Faces::want_full`): that of the face-up card the open menu is for.
#[must_use]
pub fn sheet_face(input: &ScreenIn) -> Option<FaceId> {
    let open = input.table.menu.as_ref().filter(|_| input.window.phone())?;
    preview_face(open.target, input)
}

/// The face of `target` if it is a card whose face the viewer can see.
fn preview_face(target: Target, input: &ScreenIn) -> Option<FaceId> {
    let Target::Card(slot) = target else {
        return None;
    };
    let object = (input.view?.objects.iter())
        .find(|object| object.slot == slot && object.facing == Facing::Up)?;
    input.faces?.find(object.name.as_deref()?)
}
