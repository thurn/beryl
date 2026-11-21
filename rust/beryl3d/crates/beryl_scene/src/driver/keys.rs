//! The page's keyboard and host actions: typing into a field, pressing keys and chords, and
//! answering what the client asks of the platform.

use super::{KeyChord, Page};
use crate::client;
use crate::element::Chord;
use crate::input::{Key, KeyCode, KeyPhase, TextChange};
use client::{Input, Request};

impl Page {
    /// Presses the field `locator` names and types `text` at its caret, as a desktop host
    /// sends typed characters.
    pub fn type_text(&mut self, locator: &str, text: &str) {
        self.press(locator);
        self.give(Input::Text(TextChange::Insert(text.to_owned())));
        self.render();
    }

    /// Presses and releases the keys of `binding`, written as a shortcut is, such as
    /// `Shift+ArrowLeft` or `Cmd+X`, on the platform the client was told of (the Mac until
    /// `Input::Platform` says otherwise).
    ///
    /// # Panics
    ///
    /// When the binding names a key or modifier the client's input does not carry.
    pub fn chord(&mut self, binding: &str) {
        let chord = Chord::parse(binding, self.client.mac())
            .unwrap_or_else(|| panic!("{binding:?} is not a chord"));
        for phase in [KeyPhase::Down, KeyPhase::Up] {
            self.give(Input::Key(Key {
                code: chord.key,
                phase,
                modifiers: chord.modifiers,
            }));
        }
        self.render();
    }

    /// Hands the client `input` as a host would, such as the clipboard's text for
    /// `Input::Paste`.
    pub fn give_input(&mut self, input: Input) {
        self.give(input);
        self.render();
    }

    /// What the text field named `name` holds.
    #[must_use]
    pub fn field(&self, name: &str) -> crate::widgets::FieldState {
        self.client.field(name)
    }

    /// The next thing the client asked of the platform, if any.
    pub fn next_request(&mut self) -> Option<Request> {
        self.client.next_request()
    }

    /// Presses and releases a key.
    pub fn key(&mut self, code: KeyCode) {
        self.key_chord(KeyChord::plain(code));
    }

    /// Presses and releases the key of `chord` with its modifiers held, as a `key` action does.
    pub fn key_chord(&mut self, chord: KeyChord) {
        let modifiers = chord.modifiers(self.client.mac());
        for phase in [KeyPhase::Down, KeyPhase::Up] {
            self.give(Input::Key(Key {
                code: chord.code,
                phase,
                modifiers,
            }));
        }
        self.render();
    }
}
