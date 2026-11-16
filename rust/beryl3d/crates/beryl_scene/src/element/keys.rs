//! Keyboard shortcuts (docs/beryl-3d.md, "Elements"): a screen gives the shortcuts it answers
//! beside its tree, each a binding as the web interface writes one, such as `Cmd+Shift+L`, and
//! the press it means. A binding is read by the rules of `ui/src/table/shortcuts.ts`: its
//! modifiers before the last `+`, then one key's name, matched by the physical key and every
//! modifier held. `Cmd` is the Command key on the Mac and Control elsewhere.

use super::Press;
use crate::input::{Key, KeyCode, Modifiers};

/// A physical key with the modifiers held with it: what a binding names and a key event
/// carries (`bindingChord` and `eventFlags` in shortcuts.ts).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chord {
    pub key: KeyCode,
    pub modifiers: Modifiers,
}

impl Chord {
    /// The chord `binding` names on the Mac if `mac`, or elsewhere; `None` for a binding with a
    /// modifier shortcuts.ts does not know, or a key Beryl 3D's input does not carry.
    #[must_use]
    pub fn parse(binding: &str, mac: bool) -> Option<Self> {
        // The key's name is everything after the last `+`, or `+` itself.
        let (prefix, name) = match binding.strip_suffix('+') {
            Some(rest) if rest.is_empty() || rest.ends_with('+') => (rest, "+"),
            _ => binding.rsplit_once('+').unwrap_or(("", binding)),
        };
        let mut modifiers = Modifiers::default();
        for modifier in prefix.split('+').filter(|part| !part.is_empty()) {
            let flag = match modifier {
                "Cmd" if mac => &mut modifiers.command,
                "Cmd" | "Ctrl" => &mut modifiers.control,
                "Option" | "Alt" => &mut modifiers.alt,
                "Shift" => &mut modifiers.shift,
                "Meta" => &mut modifiers.command,
                _ => return None,
            };
            *flag = true;
        }
        Some(Self {
            key: key_named(name, mac)?,
            modifiers,
        })
    }

    /// The chord a key event presses.
    #[must_use]
    pub fn of(key: &Key) -> Self {
        Self {
            key: key.code,
            modifiers: key.modifiers,
        }
    }
}

/// The key a binding names, by `codeFor` in shortcuts.ts, among the keys `KeyCode` holds.
fn key_named(name: &str, mac: bool) -> Option<KeyCode> {
    use KeyCode::{
        A, ArrowDown, ArrowLeft, ArrowRight, ArrowUp, B, Backspace, C, D, Delete, Digit0, Digit1,
        Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9, E, Enter, Escape, F, G, H,
        I, J, K, L, M, N, O, P, Q, R, S, Space, T, Tab, U, V, W, X, Y, Z,
    };
    const LETTERS: [KeyCode; 26] = [
        A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    ];
    const DIGITS: [KeyCode; 10] = [
        Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9,
    ];
    let mut chars = name.chars();
    if let (Some(only), None) = (chars.next(), chars.next()) {
        if only.is_ascii_alphabetic() {
            let index = only.to_ascii_uppercase() as usize - 'A' as usize;
            return Some(LETTERS[index]);
        }
        if let Some(digit) = only.to_digit(10) {
            return Some(DIGITS[digit as usize]);
        }
    }
    Some(match name {
        "Space" => Space,
        "Tab" => Tab,
        "Escape" => Escape,
        "Return" => Enter,
        "Backspace" => Backspace,
        // The Mac's key labeled Delete is the physical Backspace key.
        "Delete" if mac => Backspace,
        "ForwardDelete" if mac => Delete,
        "Delete" => Delete,
        "ArrowUp" => ArrowUp,
        "ArrowDown" => ArrowDown,
        "ArrowLeft" => ArrowLeft,
        "ArrowRight" => ArrowRight,
        _ => return None,
    })
}

/// The shortcuts a screen answers: bindings as the web interface writes them, each with the
/// press it means. The bindings are read when a key arrives, so one table serves the Mac and
/// other platforms.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shortcuts(pub Vec<(String, Press)>);

impl Shortcuts {
    /// The press the first binding that `key` presses means, on the Mac if `mac`.
    #[must_use]
    pub fn press_for(&self, key: &Key, mac: bool) -> Option<&Press> {
        let chord = Chord::of(key);
        (self.0.iter())
            .find(|(binding, _)| Chord::parse(binding, mac) == Some(chord))
            .map(|(_, press)| press)
    }
}
