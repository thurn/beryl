//! Routes toolkit field changes and clipboard and native entry requests.

use super::{Client, Platform};
use crate::command::Request;
use kit_ui::input::Input as DeviceInput;

use kit_ui::input::{Key, KeyCode, Pointer, TextChange};
use kit_ui::layout::Edit;
use kit_ui::widgets::FieldState;

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
        self.layer.field(name)
    }

    /// Hands the edited field text typed into it, an input method's, a phone's native field's
    /// or the clipboard's (`Input::Text` and `Input::Paste`).
    pub(super) fn type_into(&mut self, input: &DeviceInput) {
        let edit = match input {
            DeviceInput::Text(TextChange::Insert(words)) => Edit::Insert(words),
            DeviceInput::Text(TextChange::Replace(words)) => Edit::Replace(words),
            DeviceInput::Text(TextChange::Preedit(words)) => Edit::Preedit(words),
            DeviceInput::Paste(words) => Edit::Paste(words),
            _ => return,
        };
        self.edit(edit);
    }

    /// Hands `edit` to the focused text field and carries out what it asks: keeping the
    /// field's new text, and putting text on the clipboard or reading it. Returns whether the
    /// field took the edit.
    pub(super) fn edit(&mut self, edit: Edit) -> bool {
        let edited = self.layer.edit(edit);
        if let Some(change) = edited.change {
            self.feature_field_changed(change);
            self.follow_table_query();
            self.show_screen();
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
        self.layer
            .press_field(self.now, pointer.position, pointer.modifiers.shift);
    }

    /// Asks the host to begin text entry over a field that has just taken the focus, or to end
    /// it once no field has the focus: on a phone that shows or hides the native field, and on
    /// the desktop it turns the input method on or off.
    pub(super) fn follow_entry(&mut self) {
        let Some(entry) = self.layer.text_entry() else {
            return;
        };
        if entry.end {
            self.requests.push_back(Request::EndTextEntry);
        }
        if let Some((field, rect, text)) = entry.begin {
            self.requests
                .push_back(Request::BeginTextEntry { field, rect, text });
        }
    }
}
