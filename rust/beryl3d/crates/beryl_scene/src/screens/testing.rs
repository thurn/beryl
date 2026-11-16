//! The testing screen: every recording the host listed (`Input::Recordings`), a field that
//! narrows the list to the names containing what is typed in it, and a toggle for the seat a
//! recording is replayed from. A press on a recording asks the host to replay it from its start
//! (`Press::Replay`). Only a tools build of the app lists recordings, and the start screen
//! offers the way here (the key T) only once some are listed.
//!
//! The web interface's testing screen (`Testing` in `ui/src/screens/Testing.tsx`) lists cards
//! with spec moments under the title "Scenarios", filters them with a field whose placeholder
//! is "Find a card", and names the seats "You" and "Them"; this screen uses those words.

use beryl_protocol::protocol::Side;

use super::ScreenIn;
use crate::element::{Align, Element, Press, column, row, text};
use crate::frame::Role;
use crate::tokens::{color, size, space, type_style};
use crate::widgets::{button, field, list, primary};

/// The name of the field that narrows the list.
pub const FILTER: &str = "filter";

/// The name of the list of recordings.
pub const RECORDINGS: &str = "recordings";

/// The words of the seat toggle, which is on while recordings replay from the opponent's seat:
/// the web's name for that seat.
pub const THEIRS: &str = "Them";

/// What the flat layer shows on the screen.
#[must_use]
pub fn testing(input: &ScreenIn) -> Element {
    let mut state = input.fields.get(FILTER);
    "Find a card".clone_into(&mut state.placeholder);
    let query = state.text.to_lowercase();
    let filter = field(FILTER, &state).grow(1.0);
    let header = row()
        .name("header")
        .gap(space::L)
        .align_items(Align::Center)
        .child(text("Scenarios").style(type_style::TITLE))
        .child(filter)
        .child(seat_toggle(input.seat));
    let rows = (input.recordings.iter())
        .enumerate()
        .filter(|(_, name)| name.to_lowercase().contains(&query))
        .filter_map(|(index, name)| Some(recording_row(u16::try_from(index).ok()?, name)));
    column()
        .name("testing")
        .align_self(Align::Stretch)
        .grow(1.0)
        .gap(space::L)
        .padding(space::L)
        .background(color::SURFACE)
        .child(header)
        .child(list(RECORDINGS, rows).grow(1.0).basis(0.0))
}

/// The toggle for the seat recordings replay from: on, drawn as the primary button, while it
/// is the opponent's.
fn seat_toggle(seat: Side) -> Element {
    let theirs = seat == Side::Opponent;
    let (mut toggle, next) = if theirs {
        (primary(THEIRS), Side::You)
    } else {
        (button(THEIRS), Side::Opponent)
    };
    toggle.role = Role::Toggle;
    toggle.selected(theirs).on_press(Press::Seat(next))
}

/// The row of the `index`th recording, named `name`, which replays it when pressed.
fn recording_row(index: u16, name: &str) -> Element {
    let mut row = Element::new(Role::Button)
        .name(name)
        .min_height(size::MENU_ROW_HEIGHT)
        .align_items(Align::Center)
        .on_press(Press::Replay { recording: index });
    row.text = Some(name.to_owned());
    row.style.text = type_style::MENU;
    row.style.padding = [
        size::MENU_ROW_PADDING_Y,
        size::MENU_ROW_PADDING_END,
        size::MENU_ROW_PADDING_Y,
        size::MENU_ROW_PADDING_START,
    ];
    // The list's keys move through its rows.
    row.focus = true;
    row
}
