//! The client's side of editing a text field (`layout::Edit`): what it hands the edited field,
//! the text it keeps for the screens (`Fields`), and what it asks of the host, which is text
//! entry over the field and the clipboard.

use glam::Vec2;

use super::{Client, Platform};
use crate::command::Request;
use crate::element::Press;
use crate::frame::ElementId;
use crate::input::{Input, Key, KeyCode, Pointer, TextChange};
use crate::layout::Edit;
use crate::widgets::FieldState;

/// How soon after a press a second one in the same place is a double click, in nanoseconds:
/// half a second, the usual default on the desktop platforms.
const DOUBLE_CLICK: u64 = 500_000_000;

/// How far apart, in logical points, the two presses of a double click may be.
const DOUBLE_CLICK_POINTS: f32 = 4.0;

impl Client {
    /// Whether the host runs on the Mac, where `Cmd` in a shortcut is the Command key.
    #[must_use]
    pub fn mac(&self) -> bool {
        self.platform == Platform::Mac
    }

    /// What the text field named `name` holds, with its caret and selection where they are
    /// now if it is being edited. The caret moves in the field's editor without the screen
    /// being built again, so `Fields` keeps the caret of the field's last change of text.
    #[must_use]
    pub fn field(&self, name: &str) -> FieldState {
        let mut state = self.fields.get(name);
        let edited = self.layer.edited_id();
        if edited.and_then(|id| self.layer.name_of(id)) == Some(name)
            && let Some((caret, selection)) = self.layer.edited_caret()
        {
            state.caret = caret;
            state.selection = selection;
        }
        state
    }

    /// Hands the edited field text typed into it, an input method's, a phone's native field's
    /// or the clipboard's (`Input::Text` and `Input::Paste`).
    pub(super) fn type_into(&mut self, input: &Input) {
        let edit = match input {
            Input::Text(TextChange::Insert(words)) => Edit::Insert(words),
            Input::Text(TextChange::Replace(words)) => Edit::Replace(words),
            Input::Text(TextChange::Preedit(words)) => Edit::Preedit(words),
            Input::Paste(words) => Edit::Paste(words),
            _ => return,
        };
        self.edit(edit);
    }

    /// Hands `edit` to the focused text field and carries out what it asks: keeping the
    /// field's new text, and putting text on the clipboard or reading it. Returns whether the
    /// field took the edit.
    pub(super) fn edit(&mut self, edit: Edit) -> bool {
        let edited = self.layer.edit(edit);
        if let Some(text) = edited.text
            && let Some(id) = self.layer.edited_id()
        {
            self.press(&Press::Field(id, text));
        }
        if let Some(copied) = edited.copy {
            self.requests.push_back(Request::Copy(copied));
        }
        if edited.paste {
            self.requests.push_back(Request::Paste);
        }
        edited.took
    }

    /// Hands `key` to the edited field. Returns whether it means nothing else: a key the field
    /// took, or one that types a character, which the host also sends as text, so that typing
    /// into a field never sets off a screen's shortcut. Tab, Return, Escape and chords with
    /// `Cmd` go on to their usual meanings.
    pub(super) fn field_key(&mut self, key: Key) -> bool {
        let mac = self.mac();
        if self.edit(Edit::Key(key, mac)) {
            return true;
        }
        let command = if mac {
            key.modifiers.command
        } else {
            key.modifiers.control
        };
        !command && !matches!(key.code, KeyCode::Tab | KeyCode::Enter | KeyCode::Escape)
    }

    /// The primary button went down on the edited field: it moves the caret there, selects
    /// the word there on the second press of a double click, and extends the selection with
    /// Shift held.
    pub(super) fn press_field(&mut self, pointer: &Pointer) {
        let point = pointer.position;
        let word = self.last_down.is_some_and(|(at, there): (u64, Vec2)| {
            self.now.saturating_sub(at) <= DOUBLE_CLICK
                && there.distance(point) <= DOUBLE_CLICK_POINTS
        });
        // A third press starts again, as the first of a new double click.
        self.last_down = (!word).then_some((self.now, point));
        self.edit(Edit::Down {
            point,
            word,
            extend: pointer.modifiers.shift,
        });
    }

    /// Keeps `text` as what the field `id` holds, with its editor's caret and selection, and
    /// builds the screen again to show it.
    pub(super) fn keep_field(&mut self, id: ElementId, text: &str) {
        let Some(name) = self.layer.name_of(id) else {
            return;
        };
        let name = name.to_owned();
        let (caret, selection) = (self.layer.edited_id() == Some(id))
            .then(|| self.layer.edited_caret())
            .flatten()
            .unwrap_or_default();
        let state = FieldState {
            text: text.to_owned(),
            caret,
            selection,
            placeholder: String::new(),
        };
        self.fields.set(&name, state);
        self.show_screen();
    }

    /// Asks the host to begin text entry over a field that has just taken the focus, or to end
    /// it once no field has the focus: on a phone that shows or hides the native field, and on
    /// the desktop it turns the input method on or off.
    pub(super) fn follow_entry(&mut self) {
        let edited = self.layer.edited_id();
        if edited == self.entry {
            return;
        }
        if self.entry.is_some() {
            self.requests.push_back(Request::EndTextEntry);
        }
        self.entry = edited;
        if let Some((field, rect, text)) = self.layer.edited() {
            (self.requests).push_back(Request::BeginTextEntry { field, rect, text });
        }
    }
}
