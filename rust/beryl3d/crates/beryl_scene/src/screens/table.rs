//! The manual table's screen (docs/manual-multiplayer.md): the table is drawn as in a game,
//! with the life totals and piles of the hud, and the flat layer holds the table's keys and
//! the context menu a right click or a long press opens, which on a phone is a bottom sheet.
//! Only the solo table is built: the web interface's host and join screens, the command list,
//! the ask prompt and the log come later.

use beryl_protocol::protocol::Facing;
use glam::Vec2;

use super::ScreenIn;
use crate::data::space::flat::{MENU_EDGE, sheet};
use crate::element::{Align, Element, Overflow, Press, Shown, column};
use crate::frame::Role;
use crate::input::{FaceId, FaceSize};
use crate::manual::Target;
use crate::manual::commands::{COMMANDS, Scope};
use crate::manual::menus::{MENU, OpenMenu};
use crate::table::{CARD_HEIGHT, CARD_WIDTH};
use crate::widgets::{context_menu, menu};

/// What the flat layer shows on the table: a region the pointer goes through, so that it
/// reaches the table's cards, which answers every key of a table command
/// (`data/table-commands.json`, its aliases too) with `Press::Command`, and the open context
/// menu, if any.
///
/// The keys are written as the Mac's bindings: `Chord::parse` reads `Cmd` as Control and
/// `Option` as Alt elsewhere, and the file's other bindings differ from the Mac's only in
/// writing those.
#[must_use]
pub fn table(input: &ScreenIn) -> Element {
    let mut screen = column().name("table").passes(true);
    for (index, command) in COMMANDS.iter().enumerate() {
        let Ok(index) = u16::try_from(index) else {
            break;
        };
        if command.scope != Scope::Table {
            continue;
        }
        for key in command.key.iter().chain(command.key_aliases) {
            screen = screen.shortcut(key.mac, Press::Command(index));
        }
    }
    let open = input.table_menu.map(|open| {
        if input.window.phone() {
            bottom_sheet(open, input)
        } else {
            context_menu(open.at, menu(MENU, &open.title, &open.groups))
        }
    });
    screen.children(open)
}

/// The context menu on a phone, as the web's `[data-presentation="sheet"]` menu stands: along
/// the bottom of the window above its safe area, as wide as the window up to `sheet.max_width`,
/// scrolling where it is taller than the window, with a card's full face above its rows.
fn bottom_sheet(open: &OpenMenu, input: &ScreenIn) -> Element {
    let window = input.window;
    let safe = window.safe_area;
    let width =
        (window.size.width - safe.left - safe.right - 2.0 * MENU_EDGE).min(sheet::MAX_WIDTH);
    let mut panel = menu(MENU, &open.title, &open.groups)
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
    let open = input.table_menu.filter(|_| input.window.phone())?;
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
