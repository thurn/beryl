//! The manual table's screen (docs/manual-multiplayer.md): the table is drawn as in a game,
//! with the life totals and piles of the hud, and the flat layer holds the table's keys and
//! the context menu a right click opens. Only the solo table is built: the web interface's
//! host and join screens, the command list, the ask prompt and the log come later.

use super::ScreenIn;
use crate::element::{Element, Press, column};
use crate::manual::commands::{COMMANDS, Scope};
use crate::manual::menus::MENU;
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
    let open = input
        .table_menu
        .map(|open| context_menu(open.at, menu(MENU, &open.title, &open.groups)));
    screen.children(open)
}
