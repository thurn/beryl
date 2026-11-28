//! The testing screen: every recording the host listed (`Input::Recordings`), a field that
//! narrows the list to the names containing what is typed in it, and a toggle for the seat a
//! recording is replayed from. A press on a recording asks the host to replay it from its start
//! (`Message::Replay`). Only a tools build of the app lists recordings, and the start screen
//! offers the way here (the key T) only once some are listed.
//!
//! Beside them, the playground: a field that finds the card closest to what is typed in it, a
//! row for that card that lists its examples, and a row for each example that plays it. A
//! card the session could make no example of says why, or "No examples".
//!
//! The web interface's testing screen (`Testing` in `ui/src/screens/Testing.tsx`) lists cards
//! with spec moments under the title "Scenarios", filters them with a field whose placeholder
//! is "Find a card", and names the seats "You" and "Them"; this screen uses those words.

use beryl_protocol::protocol::Side;

use super::{CARD, FILTER, Inputs, Message, State};
use kit_base::tokens::{color, size, space, type_style};
use kit_frame::frame::Role;
use kit_ui::element::{Align, Element, Justify, column, row, text};
use kit_ui::widgets::{button, field, list, primary, screen::whole_window};

/// The name of the list of recordings.
pub const RECORDINGS: &str = "recordings";
/// The name of the playground's list: the card found, then its examples.
pub const PLAYGROUND: &str = "playground";
/// The words the playground's list shows for a card with no example.
pub const NO_EXAMPLES: &str = "No examples";

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
            Some(list_row(
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
            list_row(
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
            .children(show_stories.then_some(stories))
            .child(playground(testing, input, Some(layout.metrics.list_row)));
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
                    // The playground shares the right half with the stories, so that the
                    // recordings keep half the width and their long names one line.
                    .child(
                        column()
                            .grow(1.0)
                            .basis(0.0)
                            .min_width(0.0)
                            .min_height(0.0)
                            .gap(space::L)
                            .child(playground(testing, input, None))
                            .children(show_stories.then(story_heading))
                            .children(show_stories.then(|| stories.min_height(0.0))),
                    ),
            ),
    )
}

/// The playground's column: its heading, the card field, and the list of the card found and
/// the examples of the card picked. Each row is at least `minimum` tall, as the list rows are.
fn playground(testing: &State, input: &Inputs, minimum: Option<f32>) -> Element {
    let mut state = input.card.clone();
    "Play a card".clone_into(&mut state.placeholder);
    let mut field = field(CARD, &state).shrink(0.0);
    field.style.align_self = Some(Align::Stretch);
    let found = (testing.found.as_deref()).map(|card| list_row(Message::Pick, card, minimum));
    let mut notes = Vec::new();
    let mut examples = Vec::new();
    match &testing.examples {
        Some(Ok(card)) => {
            notes.extend(card.missing.iter().map(String::as_str));
            if card.examples.is_empty() {
                notes.push(NO_EXAMPLES);
            }
            examples.extend(
                (0u16..)
                    .zip(&card.examples)
                    .map(|(at, example)| list_row(Message::Play(at), &example.text(), minimum)),
            );
        }
        Some(Err(why)) => notes.push(why),
        None => {}
    }
    column()
        .name("playground column")
        .gap(space::L)
        .grow(1.0)
        .basis(0.0)
        .min_width(0.0)
        .min_height(0.0)
        .child(text("Playground").style(type_style::MENU).shrink(0.0))
        .child(field)
        .children(
            notes
                .into_iter()
                .map(|note| text(note).style(type_style::BODY).color(color::TEXT_DIM)),
        )
        // An empty list would be a focusable box smaller than a phone's touch target.
        .children((found.is_some() || !examples.is_empty()).then(|| {
            list(PLAYGROUND, found.into_iter().chain(examples))
                .grow(1.0)
                .basis(0.0)
                .min_height(0.0)
        }))
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
pub(super) fn list_row(
    message: impl Clone + PartialEq + std::fmt::Debug + 'static,
    name: &str,
    minimum: Option<f32>,
) -> Element {
    let least = minimum.unwrap_or(size::MENU_ROW_HEIGHT);
    // A long name breaks onto more lines, so the row keeps the height its text takes rather
    // than shrinking to fit more rows in the list.
    // `type_style::MENU` has a line height of 19.6 points, so a name that broke onto two lines
    // made a row 45.2 points tall and put the rows after it at fractions of a point, where the
    // `overlap` check found neighbors cutting into each other. `LIST_ROW` is exactly 21 points a line.
    let mut row = Element::new(Role::Button)
        .name(name)
        .min_height(least)
        .shrink(0.0)
        .align_items(Align::Center)
        .on_press(message)
        .child(
            text(name)
                .style(type_style::LIST_ROW)
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
