//! The keys the player has bound to the manual table's commands (docs/manual-multiplayer.md,
//! "The command list"): Option+Return on a row of the command list waits for a key and binds
//! it to that row's command, taking it from any other command that had it, and the 'Reset
//! shortcuts' row of the list opened by Shortcuts puts every command back on its own keys. The
//! bindings are kept with the player's settings. This is the web's `Bindings`
//! (`ui/src/table/bindings.ts`) with `bindingFor` (`ui/src/table/shortcuts.ts`).

use std::collections::BTreeMap;

use super::commands::{COMMANDS, Command, Scope};
use kit_ui::element::Chord;
use kit_ui::input::{Key, KeyCode};

/// The commands whose keys the player changed, by command id: the binding each now has, as the
/// web writes one (`Cmd+Shift+L`), or `None` for a command that lost its keys to another.
/// Every other command keeps its keys from `data/table-commands.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bindings(BTreeMap<String, Option<String>>);

impl Bindings {
    /// No command's keys changed.
    #[must_use]
    pub const fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// The bindings of `overrides` that name a command of the table and, where they give a
    /// binding, one Beryl 3D can read; any other is dropped, as the web drops a stored entry it
    /// cannot use.
    #[must_use]
    pub fn from_overrides(overrides: BTreeMap<String, Option<String>>) -> Self {
        let usable = |id: &str, binding: &Option<String>| {
            COMMANDS.iter().any(|command| command.id == id)
                && binding.as_deref().is_none_or(|binding| {
                    Chord::parse(binding, true).is_some() || Chord::parse(binding, false).is_some()
                })
        };
        Self(
            (overrides.into_iter())
                .filter(|(id, binding)| usable(id, binding))
                .collect(),
        )
    }

    /// The commands whose keys changed, as `from_overrides` takes them back.
    #[must_use]
    pub fn overrides(&self) -> &BTreeMap<String, Option<String>> {
        &self.0
    }

    /// The binding a menu row or the command list shows for `command` on the Mac if `mac`, or
    /// elsewhere: the one the player gave it, or its own (`Bindings.get`).
    #[must_use]
    pub fn get(&self, command: &Command, mac: bool) -> Option<&str> {
        match self.0.get(command.id) {
            Some(binding) => binding.as_deref(),
            None => command.key.map(|key| if mac { key.mac } else { key.other }),
        }
    }

    /// Every binding that gives `command`, as the table's keys answer them: the player's, or
    /// else its own key and its aliases, written as the Mac's (`Chord::parse` reads `Cmd` as
    /// Control elsewhere).
    pub fn keys(&self, command: &'static Command) -> impl Iterator<Item = &str> {
        let given = self.0.get(command.id);
        let own = (given.is_none())
            .then(|| {
                command
                    .key
                    .iter()
                    .chain(command.key_aliases)
                    .map(|key| key.mac)
            })
            .into_iter()
            .flatten();
        given.and_then(Option::as_deref).into_iter().chain(own)
    }

    /// The id of the command of `scope` that the key `chord` gives on the Mac if `mac`, if any
    /// (`Bindings.lookup`).
    #[must_use]
    pub fn owner(&self, scope: Scope, chord: Chord, mac: bool) -> Option<&'static str> {
        // A binding the player gave wins over a command's own key, as the web adds them last.
        let given = (COMMANDS.iter()).find(|command| {
            command.scope == scope
                && (self.0.get(command.id)).is_some_and(|binding| {
                    (binding.as_deref())
                        .is_some_and(|binding| Chord::parse(binding, mac) == Some(chord))
                })
        });
        let own = || {
            (COMMANDS.iter()).find(|command| {
                command.scope == scope
                    && !self.0.contains_key(command.id)
                    && (command.key.iter().chain(command.key_aliases))
                        .any(|key| Chord::parse(key.mac, mac) == Some(chord))
            })
        };
        given.or_else(own).map(|command| command.id)
    }

    /// Binds `binding` to `command` on the Mac if `mac`, taking it from the command that had
    /// it, whose id is returned (`Bindings.bind`). Nothing for a binding Beryl 3D cannot read.
    pub fn bind(&mut self, command: &Command, binding: &str, mac: bool) -> Option<&'static str> {
        let chord = Chord::parse(binding, mac)?;
        let displaced = self
            .owner(command.scope, chord, mac)
            .filter(|owner| *owner != command.id);
        if let Some(owner) = displaced {
            self.0.insert(owner.to_owned(), None);
        }
        self.0
            .insert(command.id.to_owned(), Some(binding.to_owned()));
        displaced
    }

    /// Puts every command back on its own keys (`Bindings.reset`).
    pub fn reset(&mut self) {
        self.0.clear();
    }
}

/// The binding a press of `key` makes, written for the Mac if `mac` as the web writes one
/// (`bindingFor`): the modifiers held, in the order Cmd (Meta elsewhere), Ctrl, Option (Alt),
/// Shift, then the key's name. `None` for a key that has no name in a binding, such as a
/// modifier alone.
#[must_use]
pub fn binding_for(key: &Key, mac: bool) -> Option<String> {
    let name = key_name(key.code, mac)?;
    let held = key.modifiers;
    let parts = [
        (held.command, if mac { "Cmd" } else { "Meta" }),
        (held.control, "Ctrl"),
        (held.alt, if mac { "Option" } else { "Alt" }),
        (held.shift, "Shift"),
    ];
    let mut words: Vec<&str> = (parts.iter())
        .filter(|(on, _)| *on)
        .map(|(_, word)| *word)
        .collect();
    words.push(name);
    Some(words.join("+"))
}

/// The name a binding gives the physical key `code` (`nameFor` in shortcuts.ts): its letter
/// or digit, `F1` to `F12`, or the name `Chord::parse` reads back as that key.
fn key_name(code: KeyCode, mac: bool) -> Option<&'static str> {
    use KeyCode::{
        A, ArrowDown, ArrowLeft, ArrowRight, ArrowUp, B, Back, Backslash, Backspace, BracketLeft,
        BracketRight, C, Comma, D, Delete, Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6,
        Digit7, Digit8, Digit9, E, End, Enter, Equal, Escape, F, F1, F2, F3, F4, F5, F6, F7, F8,
        F9, F10, F11, F12, G, H, Home, I, J, K, L, M, Minus, N, O, P, Period, Q, R, S, Slash,
        Space, T, Tab, U, V, W, X, Y, Z,
    };
    Some(match code {
        A => "A",
        B => "B",
        C => "C",
        D => "D",
        E => "E",
        F => "F",
        G => "G",
        H => "H",
        I => "I",
        J => "J",
        K => "K",
        L => "L",
        M => "M",
        N => "N",
        O => "O",
        P => "P",
        Q => "Q",
        R => "R",
        S => "S",
        T => "T",
        U => "U",
        V => "V",
        W => "W",
        X => "X",
        Y => "Y",
        Z => "Z",
        Digit0 => "0",
        Digit1 => "1",
        Digit2 => "2",
        Digit3 => "3",
        Digit4 => "4",
        Digit5 => "5",
        Digit6 => "6",
        Digit7 => "7",
        Digit8 => "8",
        Digit9 => "9",
        F1 => "F1",
        F2 => "F2",
        F3 => "F3",
        F4 => "F4",
        F5 => "F5",
        F6 => "F6",
        F7 => "F7",
        F8 => "F8",
        F9 => "F9",
        F10 => "F10",
        F11 => "F11",
        F12 => "F12",
        Space => "Space",
        Tab => "Tab",
        Escape => "Escape",
        Enter => "Return",
        // The Mac's key labeled Delete is the physical Backspace key.
        Backspace if mac => "Delete",
        Delete if mac => "ForwardDelete",
        Backspace => "Backspace",
        Delete => "Delete",
        ArrowUp => "ArrowUp",
        ArrowDown => "ArrowDown",
        ArrowLeft => "ArrowLeft",
        ArrowRight => "ArrowRight",
        Home => "Home",
        End => "End",
        Minus => "−",
        Equal => "=",
        Comma => ",",
        Period => ".",
        Slash => "/",
        BracketLeft => "[",
        BracketRight => "]",
        Backslash => "\\",
        // The phone's back button has no name in a binding.
        Back => return None,
    })
}
