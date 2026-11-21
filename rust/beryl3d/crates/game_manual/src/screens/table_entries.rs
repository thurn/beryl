//! The start screen's ways onto a manual table, as the web's `TableEntries`
//! (`ui/src/table/lobby/TableEntries.tsx`) shows them: 'Host a table' (not on a phone, which
//! cannot host), 'Join a table' and 'Solo table'; the host or join form under them, with the
//! player's name and, to join, the invite code; each saved table with 'Resume table'; and why
//! the last attempt to open a table failed, such as a version mismatch.
//!
//! Beryl 3D's session is built without internet play, so a hosted table is always on this
//! computer's network, as the web's browser build hosts one, and the form offers no choice of
//! connection.

use crate::features::lobby::{EntryMessage, Message};
use beryl_protocol::ipc::SavedTable;

use kit_base::tokens::type_style::START_LABEL as LABEL;
use kit_base::tokens::{color, space, type_style};
use kit_text::text::TypeStyle;
use kit_ui::element::{Align, Element, column, row, text};
use kit_ui::widgets::screen::ScreenColumn;
use kit_ui::widgets::{Fields, button, field, primary};

/// The name of the host or join form's name field.
pub const NAME: &str = "table name";

/// The name of the join form's invite code field.
pub const INVITE: &str = "invite code";

/// What the web says when a table did not open and its session gave no reason.
pub const COULD_NOT_OPEN: &str = "Could not open the table.";

impl TableEntries {
    /// No form open, no saved tables and no error, as the start screen first opens.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            mode: None,
            saved: Vec::new(),
            saved_error: false,
            error: None,
            asked: false,
        }
    }
}

/// Body text: 15-point sans, for the saved tables and the error.
const BODY: TypeStyle = type_style::SCREEN_BODY;

/// Which form the start screen shows under its table buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryMode {
    Host,
    Join,
}

/// What the start screen's table entries hold. The client keeps it while the screen is built
/// again; the fields' text is in `Fields`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TableEntries {
    /// The form open under the buttons, if any.
    pub mode: Option<EntryMode>,
    /// The tables that can be resumed, the last joined table first.
    pub saved: Vec<SavedTable>,
    /// Whether the session could not read the saved tables.
    pub saved_error: bool,
    /// Why the last table did not open, which the screen shows until the next attempt.
    pub error: Option<String>,
    /// Whether the saved tables have been asked for since the start screen last opened.
    pub asked: bool,
}

/// The table buttons, the open form, the saved tables and the last error, laid out for the
/// desktop's panel or, with `phone`, the phone's column, whose buttons fill its width.
#[must_use]
pub fn entries(
    entries: &TableEntries,
    fields: &Fields,
    layout: Option<ScreenColumn>,
) -> Vec<Element> {
    // Three buttons at the usual least width are wider than the desktop panel, so each takes
    // its words' width there, as the web's `.lobby-actions` buttons do.
    let action = |mut button: Element| {
        if let Some(layout) = layout {
            layout.action(button)
        } else {
            button.style.min_width = None;
            button
        }
    };
    let mut buttons = (if layout.is_some() { column() } else { row() })
        .name("manual tables")
        .gap(space::M)
        .align_self(if layout.is_some() {
            Align::Stretch
        } else {
            Align::Center
        });
    if layout.is_none() {
        buttons = buttons.child(action(
            button("Host a table").on_press(Message::Entry(EntryMessage::Host)),
        ));
    }
    buttons = buttons
        .child(action(
            button("Join a table").on_press(Message::Entry(EntryMessage::Join)),
        ))
        .child(action(
            button("Solo table").on_press(Message::Entry(EntryMessage::Solo)),
        ));
    let mut shown = vec![buttons];
    shown.extend(entries.mode.map(|mode| form(fields, mode, layout)));
    if !entries.saved.is_empty() {
        let rows = (0u16..).zip(&entries.saved).map(|(at, saved)| {
            let names = match &saved.guest {
                Some(guest) => format!("{} · {guest}", saved.host),
                None => saved.host.clone(),
            };
            row()
                .name(&names)
                .gap(space::M)
                .align_items(Align::Center)
                .child(text(&names).style(BODY).color(color::TYPE).grow(1.0))
                .child(button("Resume table").on_press(Message::Entry(EntryMessage::Resume(at))))
        });
        shown.push(
            column()
                .name("Saved tables")
                .gap(space::S)
                .align_self(Align::Stretch)
                .shrink(0.0)
                .children(rows),
        );
    }
    if entries.saved_error {
        shown.push(
            text("Could not load saved tables.")
                .style(BODY)
                .color(color::TEXT_DIM),
        );
    }
    shown.extend(
        (entries.error.as_deref())
            .map(|error| text(error).style(BODY).color(color::EMBER).shrink(0.0)),
    );
    shown
}

/// The host or join form: the name, the invite code to join, the button that opens the table
/// once the fields hold what it needs, and Cancel.
fn form(fields: &Fields, mode: EntryMode, layout: Option<ScreenColumn>) -> Element {
    let labelled = |label: &str, name: &str| {
        let mut field = field(name, &fields.get(name));
        field.style.align_self = Some(Align::Stretch);
        field.style.shrink = 0.0;
        [text(label).style(LABEL).color(color::TEXT_DIM), field]
    };
    let name = fields.get(NAME).text.trim().to_owned();
    let invite = fields.get(INVITE).text.trim().to_owned();
    let name_ok = !name.is_empty();
    let (label, ready) = match mode {
        EntryMode::Host => ("Host", name_ok),
        EntryMode::Join => ("Join", name_ok && !invite.is_empty()),
    };
    let open = primary(label)
        .enabled(ready)
        .on_press(Message::Entry(EntryMessage::OpenTable { name, invite }));
    let cancel = button("Cancel").on_press(Message::Entry(EntryMessage::CancelEntry));
    let buttons = if let Some(layout) = layout {
        column()
            .gap(space::M)
            .align_self(Align::Stretch)
            .child(layout.action(open))
            .child(layout.action(cancel))
    } else {
        row().gap(space::M).child(open).child(cancel)
    };
    column()
        .name(if mode == EntryMode::Host {
            "host form"
        } else {
            "join form"
        })
        .gap(space::S)
        .align_self(Align::Stretch)
        .shrink(0.0)
        .children(labelled("Name", NAME))
        .children(
            (mode == EntryMode::Join)
                .then(|| labelled("Invite code", INVITE))
                .into_iter()
                .flatten(),
        )
        .child(buttons.name("form buttons"))
}

/// The command of `data/table-commands.json` whose keys open the host form (Cmd+N), as the
/// web's start screen binds it on the desktop.
pub const HOST: &str = "lobby.host";

/// What a key of `HOST` sends.
#[must_use]
pub fn host_press() -> Message {
    Message::Entry(EntryMessage::Host)
}
