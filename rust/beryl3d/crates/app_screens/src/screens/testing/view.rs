//! The testing screen: every recording the host listed (`Input::Recordings`), a field that
//! narrows the list to the names containing what is typed in it, and a toggle for the seat a
//! recording is replayed from. A press on a recording asks the host to replay it from its start
//! (`Message::Replay`). Only a tools build of the app lists recordings, and the start screen
//! offers the way here (the key T) only once some are listed.
//!
//! The web interface's testing screen (`Testing` in `ui/src/screens/Testing.tsx`) lists cards
//! with spec moments under the title "Scenarios", filters them with a field whose placeholder
//! is "Find a card", and names the seats "You" and "Them"; this screen uses those words.

use beryl_protocol::protocol::Side;

use super::{FILTER, Inputs, Message, State};
use kit_base::tokens::{color, size, space, type_style};
use kit_frame::frame::Role;
use kit_ui::element::{Align, Element, Justify, column, row, text};
use kit_ui::widgets::{button, field, list, primary, screen::whole_window};

/// The name of the list of recordings.
pub const RECORDINGS: &str = "recordings";

/// The words of the seat toggle, which is on while recordings replay from the opponent's seat:
/// the web's name for that seat.
pub const THEIRS: &str = "Them";

/// What the flat layer shows on the screen.
#[must_use]
pub(super) fn testing(testing: &State, input: &Inputs) -> Element {
    let mut state = input.filter.clone();
    "Find a card".clone_into(&mut state.placeholder);
    let query = testing.filter.to_lowercase();
    let layout = input.column;
    let filter = field(FILTER, &state).grow(1.0);
    let rows = (testing.recordings.iter())
        .enumerate()
        .filter(|(_, name)| name.to_lowercase().contains(&query))
        .filter_map(|(index, name)| {
            Some(recording_row(
                Message::Replay {
                    recording: u16::try_from(index).ok()?,
                },
                name,
                layout.map(|layout| layout.metrics.list_row),
            ))
        });
    let stories = testing
        .stories
        .iter()
        .filter(|name| name.to_lowercase().contains(&query))
        .map(|name| {
            recording_row(
                Message::Story(name.clone()),
                name,
                layout.map(|layout| layout.metrics.list_row),
            )
        });
    let stories = list("Stories", stories).grow(1.0).basis(0.0);
    let story_heading = || text("Stories").style(type_style::MENU).shrink(0.0);
    let show_stories = !testing.stories.is_empty();
    let rows = list(RECORDINGS, rows).grow(1.0).basis(0.0);
    if let Some(layout) = layout {
        // One safe-area column: the title beside the seat toggle, the field under them as wide
        // as the column, then the list, whose long names break onto more lines.
        let header = row()
            .name("header")
            .gap(space::L)
            .align_items(Align::Center)
            .justify_content(Justify::SpaceBetween)
            .shrink(0.0)
            .child(text("Scenarios").style(type_style::TITLE))
            .child(seat_toggle(testing.seat));
        let filter = filter
            .grow(0.0)
            .height(layout.metrics.start.field)
            .align_self(Align::Stretch)
            .shrink(0.0);
        let content = column()
            .name("testing column")
            .gap(space::L)
            .child(header)
            .child(filter)
            .child(rows)
            .children(show_stories.then(story_heading))
            .children(show_stories.then_some(stories));
        return layout
            .scrolling("testing", input.window, content)
            .background(color::SURFACE);
    }
    let header = row()
        .name("header")
        .gap(space::L)
        .align_items(Align::Center)
        .child(text("Scenarios").style(type_style::TITLE))
        .child(filter)
        .child(seat_toggle(testing.seat));
    whole_window(
        input.window.size,
        column()
            .name("testing")
            .gap(space::L)
            .padding(space::L)
            .background(color::SURFACE)
            .child(header.shrink(0.0))
            .child(
                row()
                    .grow(1.0)
                    .basis(0.0)
                    .min_height(0.0)
                    .gap(space::L)
                    .child(rows.min_width(0.0).min_height(0.0))
                    .children(show_stories.then(|| {
                        column()
                            .grow(1.0)
                            .basis(0.0)
                            .min_width(0.0)
                            .min_height(0.0)
                            .child(story_heading())
                            .child(stories.min_height(0.0))
                    })),
            ),
    )
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
    toggle.selected(theirs).on_press(Message::Seat(next))
}

/// The row of the `index`th recording, named `name`, which replays it when pressed.
/// On a phone the row is a touch target's height at the least.
fn recording_row(message: Message, name: &str, minimum: Option<f32>) -> Element {
    let least = minimum.unwrap_or(size::MENU_ROW_HEIGHT);
    // A long name breaks onto more lines, so the row keeps the height its text takes rather
    // than shrinking to fit more rows in the list.
    let mut row = Element::new(Role::Button)
        .name(name)
        .min_height(least)
        .shrink(0.0)
        .align_items(Align::Center)
        .on_press(message)
        .child(
            text(name)
                .style(type_style::MENU)
                .color(color::TYPE)
                .grow(1.0)
                .shrink(1.0)
                .passes(true),
        );
    // The phone's row is a touch target's height already, so it needs no padding above and
    // below; its lines, a whole number of points tall, then make a row of whole points, which
    // keeps rows from overlapping by a fraction.
    let padding_y = if minimum.is_some() {
        0.0
    } else {
        size::MENU_ROW_PADDING_Y
    };
    row.style.padding = [
        padding_y,
        size::MENU_ROW_PADDING_END,
        padding_y,
        size::MENU_ROW_PADDING_START,
    ];
    // The list's keys move through its rows.
    row.focus = true;
    row
}
