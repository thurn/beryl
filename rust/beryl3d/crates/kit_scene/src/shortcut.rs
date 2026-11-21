//! Keyboard shortcuts as data (docs/beryl-3d-architecture.md, "Toolkit capabilities"): each
//! feature declares the shortcuts it answers on its registry entry, the keys and what they do,
//! and the client's router sends a key that no focused element took to the feature that
//! declared it. The registry can then list every shortcut, and a menu row or tooltip can show a
//! shortcut's keys from the same declaration (`Shortcut::describe`).

use kit_ui::element::{Chord, describe};

/// Where a declared shortcut's keys come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keys {
    /// One binding, written as the web interface writes one for the Mac, such as
    /// `Cmd+Shift+B` (`Chord::parse` reads `Cmd` as Control elsewhere).
    Binding(&'static str),
    /// The keys of the app's command with this id, which a player may bind to other keys: its
    /// own key and aliases, or the player's binding.
    Command(&'static str),
    /// Every command of the app's group of commands with this name, each on its keys as
    /// `Command` finds them.
    Commands(&'static str),
}

/// When a declared shortcut is asked about a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// While its feature is shown. These are asked first.
    Shown,
    /// On every screen, after the shortcuts of the features shown and after Escape has closed
    /// the sheet or menu drawn over the rest.
    App,
}

/// A keyboard shortcut a feature declares: its keys, the words that name what it does, and
/// when it is asked. The feature turns a press of it into a message (`Pressed`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub keys: Keys,
    pub label: &'static str,
    pub reach: Reach,
}

impl Shortcut {
    /// The shortcut `binding`, which does `label` while its feature is shown.
    #[must_use]
    pub const fn binding(binding: &'static str, label: &'static str) -> Self {
        Self {
            keys: Keys::Binding(binding),
            label,
            reach: Reach::Shown,
        }
    }

    /// The keys of the command `id`, which does `label` while its feature is shown.
    #[must_use]
    pub const fn command(id: &'static str, label: &'static str) -> Self {
        Self {
            keys: Keys::Command(id),
            label,
            reach: Reach::Shown,
        }
    }

    /// Every command of the group `name`, each on its own keys, while the feature is shown.
    #[must_use]
    pub const fn commands(name: &'static str, label: &'static str) -> Self {
        Self {
            keys: Keys::Commands(name),
            label,
            reach: Reach::Shown,
        }
    }

    /// The same shortcut, answered on every screen (`Reach::App`).
    #[must_use]
    pub const fn on_every_screen(self) -> Self {
        Self {
            reach: Reach::App,
            ..self
        }
    }

    /// Whether `chord` presses this shortcut's binding on the Mac if `mac`. Always `false` for
    /// command keys, which the app looks up in its list of commands.
    #[must_use]
    pub fn pressed_by(&self, chord: Chord, mac: bool) -> bool {
        match self.keys {
            Keys::Binding(binding) => Chord::parse(binding, mac) == Some(chord),
            Keys::Command(_) | Keys::Commands(_) => false,
        }
    }

    /// The words a menu row or tooltip shows for this shortcut's binding on the Mac if `mac`,
    /// such as `Shift+Cmd+B` (`element::describe`); empty for command keys, whose binding the
    /// app's list of commands holds.
    #[must_use]
    pub fn describe(&self, mac: bool) -> String {
        match self.keys {
            Keys::Binding(binding) => describe(binding, mac),
            Keys::Command(_) | Keys::Commands(_) => String::new(),
        }
    }
}

/// A key that pressed a declared shortcut: the shortcut's index in its feature's list, the key
/// with its modifiers, which platform's bindings it was read by, and whether it is the key's
/// first press rather than a repeat the host sends while it is held. For `Keys::Commands` the
/// feature looks the chord up among its commands itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pressed {
    pub index: usize,
    pub chord: Chord,
    pub mac: bool,
    pub first: bool,
}
