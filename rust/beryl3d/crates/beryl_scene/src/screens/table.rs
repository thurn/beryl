//! The manual table's screen (docs/manual-multiplayer.md): the table is drawn as in a game,
//! with the life totals and piles of the hud, and the flat layer holds the table's keys, the
//! log at its left, and whatever the table shows over itself: the context menu a right click or
//! a long press opens, which on a phone is a bottom sheet, the command list, the ask prompt in
//! the dock, the token search or the library order tray; the viewer of a pile; the rectangle a
//! sweep draws; the summary of the selected permanents; the outline of where a held card would
//! land; and what the assists show: the proposal panel, state nudges' labels and the sorting
//! tray (`manual::assists`); the guided start's opening (`super::table_opening`); and, while
//! the other seat's connection is down, the banner that says so (`manual::link`). Only the solo
//! table can be opened: the web interface's host and join screens come later.

use beryl_protocol::protocol::Facing;
use glam::Vec2;

use super::ScreenIn;
use crate::data::space::flat::{MENU_EDGE, sheet};
use crate::data::space::pointer::DRAG_THRESHOLD_POINTS;
use crate::element::{Align, Element, Overflow, Press, Shown, column};
use crate::frame::Role;
use crate::hud::{players, steps};
use crate::input::{FaceId, FaceSize};
use crate::inspect::viewer::table_viewer;
use crate::manual::Target;
use crate::manual::commands::{COMMANDS, Scope};
use crate::manual::menus::{MENU, OpenMenu};
use crate::manual::{
    TableState, ask, assists, drop, link, log, opening, order, palette, pools, quit, selection,
    tokens, turn,
};
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
    let frozen = input.view.is_some_and(link::frozen);
    // A frozen table answers no key of its own (`manual::link`).
    for (index, command) in COMMANDS.iter().enumerate().filter(|_| !frozen) {
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
    for (index, command) in COMMANDS.iter().enumerate().filter(|_| !frozen) {
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
    // A frozen table hides its log, whose undo and rewind are table input, and whose button
    // on the phone stands where the banner does. The guided start's opening covers it.
    let opening = input
        .view
        .is_some_and(|view| opening::prompt(view).is_some());
    let log = lines.filter(|_| !frozen && !opening).and_then(|lines| {
        log::log(
            lines,
            state.log_open,
            &state.blocked,
            input.window.phone().then_some(input.window),
        )
    });
    let table = input.view.and_then(|view| view.table.as_ref());
    let proposals = table.and_then(|table| assists::proposals(&table.proposals, input.window));
    let labels = (input.view.zip(table))
        .map(|(view, table)| assists::labels(&table.labels, view, input.window.phone()))
        .unwrap_or_default();
    let sort = input
        .view
        .and_then(|view| assists::sort_tray(view, input.window.size));
    let open = state.menu.as_ref().map(|open| {
        if input.window.phone() {
            bottom_sheet(open, input)
        } else {
            context_menu(open.at, menu(MENU, &open.title, &open.whose, &open.groups))
        }
    });
    let sweep = (state.sweep)
        .filter(|(from, to)| from.distance(*to) >= DRAG_THRESHOLD_POINTS)
        .map(|(from, to)| selection::rectangle(selection::sweep_box(from, to)));
    let landing = state.landing.as_ref().map(drop::target);
    // The summary hides while a sweep is drawn, as the web's hides while selecting, and while
    // the command list, a prompt or the order tray is open over the table, or the table is frozen.
    let summary = (input.view)
        .filter(|_| state.sweep.is_none() && !state.overlay_open() && !frozen)
        .and_then(|view| selection::summary(view, &state.selection, state.summary_over));
    let window = input.window;
    let connection = input.view.and_then(|view| {
        let top = link::top(window.size, window.phone(), window.safe_area.top);
        link::banner(view, window.size, top)
    });
    screen
        .children(controls(state, input, frozen))
        .children(log)
        .children(summary)
        .children(proposals)
        .children(labels)
        .children(sweep)
        .children(landing)
        .children(table_viewer(input))
        .children(sort)
        .children(open)
        .children(over(state, input))
        .children(super::table_opening::table_opening(input))
        .children(connection)
}

/// The life orbs, the step tracks, the rows of mana and player counters and the Next button,
/// none while the guided start's screen covers the table, as the game's opening covers its
/// controls. The Next button stands in the lower right, where the ask prompt docks, so it gives
/// way to whatever the table shows over itself, and a `frozen` table shows none.
fn controls(state: &TableState, input: &ScreenIn, frozen: bool) -> Vec<Element> {
    if input
        .view
        .and_then(crate::manual::opening::prompt)
        .is_some()
    {
        return Vec::new();
    }
    let next = (input.view.and_then(|view| view.table.as_ref()))
        .filter(|_| state.menu.is_none() && !state.overlay_open() && !frozen)
        .map(|table| {
            let inset = crate::data::space::flat::table::EDGE;
            turn::next(table.phase, input.window.size, inset)
        });
    let pools =
        (input.view).and_then(|view| pools::pools(view, input.window.size, input.window.phone()));
    [
        players::players(input),
        steps::table_steps(input),
        pools,
        next,
    ]
    .into_iter()
    .flatten()
    .collect()
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
    if state.quitting {
        return Some(quit::question(window));
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
    let mut panel = sheet_menu(MENU, &open.title, &open.whose, &open.groups, open.page)
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
