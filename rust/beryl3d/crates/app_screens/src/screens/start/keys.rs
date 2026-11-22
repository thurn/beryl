//! The start screen's keyboard shortcuts, as its registry entry declares them: Enter does what
//! the primary button does (back to the game in progress, else a new one), G opens the gallery,
//! T opens the testing screen in a tools build of the app, which lists the recordings, and on
//! the desktop the keys of `lobby.host` (Cmd+N) open the host form, as the web's start screen
//! binds them. On the gallery, which the start screen opens, Escape closes an open menu, then
//! the command list's preview, then goes back to the start screen.

use beryl_protocol::SessionId;
use game_manual::screens::table_entries::{HOST, host_press};
use kit_scene::Shortcut;
use kit_ui::element::Action;

use super::Message;
use crate::screens::{GalleryState, Screen, gallery_escape};

/// The shortcuts, in the order `press` reads their indexes.
pub const SHORTCUTS: [Shortcut; 5] = [
    Shortcut::binding("Enter", "Play"),
    Shortcut::binding("G", "Gallery"),
    Shortcut::binding("T", "Testing"),
    Shortcut::command(HOST, "Host a table"),
    Shortcut::binding("Escape", "Back"),
];

/// What the start screen and the gallery show that their keys read.
#[derive(Clone, Copy, Debug)]
pub struct KeyInputs {
    pub screen: Screen,
    /// Whether an engine game goes on (`in_progress`).
    pub in_progress: bool,
    pub session: Option<SessionId>,
    /// Whether the app lists recordings, as a tools build does.
    pub has_recordings: bool,
    /// Whether the start screen is laid out in a fitted column (`Inputs::column`), as on a
    /// phone, where it has no host key.
    pub in_column: bool,
    pub gallery: GalleryState,
}

/// What a press of the shortcut at `index` of `SHORTCUTS` sends, or `None` where it does
/// nothing on the screen shown.
#[must_use]
pub fn press(index: usize, input: KeyInputs) -> Option<Action> {
    let start = input.screen == Screen::Start;
    Some(match index {
        0 if start && input.in_progress => Action::new(Message::Open(Screen::Game)),
        0 if start => Action::new(Message::Start(input.session.filter(|_| input.in_progress))),
        1 if start => Action::new(Message::Open(Screen::Gallery)),
        2 if start && input.has_recordings => Action::new(Message::Open(Screen::Testing)),
        3 if start && !input.in_column => Action::new(host_press()),
        4 if input.screen == Screen::Gallery => gallery_escape(input.gallery),
        _ => return None,
    })
}
