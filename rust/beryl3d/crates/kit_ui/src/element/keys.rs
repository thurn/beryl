//! Keyboard shortcuts (docs/beryl-3d.md, "Elements"): the keys a binding names, as the web
//! interface writes one, such as `Cmd+Shift+L`, and the words that show it. Features declare
//! their shortcuts with these bindings (`kit_scene::Shortcut`). A binding is read by the rules of `ui/src/table/shortcuts.ts`: its
//! modifiers before the last `+`, then one key's name, matched by the physical key and every
//! modifier held. `Cmd` is the Command key on the Mac and Control elsewhere.

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
        let (prefix, name) = split(binding);
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
        // `+`, `<`, `>` and `?` are typed with Shift on the key that also types `=`, `,`, `.`
        // and `/` (`shiftedPunctuation` in shortcuts.ts).
        let shifted = match name {
            "+" => Some(KeyCode::Equal),
            "<" => Some(KeyCode::Comma),
            ">" => Some(KeyCode::Period),
            "?" => Some(KeyCode::Slash),
            _ => None,
        };
        if shifted.is_some() {
            modifiers.shift = true;
        }
        Some(Self {
            key: match shifted {
                Some(key) => key,
                None => key_named(name, mac)?,
            },
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

/// The words that show `binding` beside a command, such as a menu row's key hint, on the Mac
/// if `mac`, by `describe` in shortcuts.ts: the modifiers in the order Control, Option, Shift,
/// Command, then the key's name, as `Ctrl+Option+Shift+Cmd+Z` on the Mac and `Ctrl+Shift+Z`
/// elsewhere. The Mac's key symbols (⌃⌥⇧⌘⌫) are spelled out because no bundled font has them. Empty for
/// a binding shortcuts.ts cannot read.
#[must_use]
pub fn describe(binding: &str, mac: bool) -> String {
    let (prefix, name) = split(binding);
    let (mut control, mut option, mut shift, mut meta) = (false, false, false, false);
    for modifier in prefix.split('+').filter(|part| !part.is_empty()) {
        let flag = match modifier {
            "Cmd" if mac => &mut meta,
            "Cmd" | "Ctrl" => &mut control,
            "Option" | "Alt" => &mut option,
            "Shift" => &mut shift,
            "Meta" => &mut meta,
            _ => return String::new(),
        };
        *flag = true;
    }
    if name.is_empty() {
        return String::new();
    }
    let labels = [
        (control, "Ctrl"),
        (option, if mac { "Option" } else { "Alt" }),
        (shift, "Shift"),
        (meta, if mac { "Cmd" } else { "Meta" }),
        (true, name),
    ];
    let shown = labels
        .iter()
        .filter(|(held, _)| *held)
        .map(|(_, label)| *label);
    shown.collect::<Vec<_>>().join("+")
}

/// A binding's modifiers, everything before the last `+`, and its key's name, everything
/// after it, or `+` itself.
fn split(binding: &str) -> (&str, &str) {
    match binding.strip_suffix('+') {
        Some(rest) if rest.is_empty() || rest.ends_with('+') => (rest, "+"),
        _ => binding.rsplit_once('+').unwrap_or(("", binding)),
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
        "Home" => KeyCode::Home,
        "End" => KeyCode::End,
        "PageUp" => KeyCode::PageUp,
        "PageDown" => KeyCode::PageDown,
        "F1" => KeyCode::F1,
        "F2" => KeyCode::F2,
        "F3" => KeyCode::F3,
        "F4" => KeyCode::F4,
        "F5" => KeyCode::F5,
        "F6" => KeyCode::F6,
        "F7" => KeyCode::F7,
        "F8" => KeyCode::F8,
        "F9" => KeyCode::F9,
        "F10" => KeyCode::F10,
        "F11" => KeyCode::F11,
        "F12" => KeyCode::F12,
        // The web writes the minus key as `−` (U+2212) in a binding and reads `-` too.
        "−" | "-" => KeyCode::Minus,
        "=" => KeyCode::Equal,
        "," => KeyCode::Comma,
        "." => KeyCode::Period,
        "/" => KeyCode::Slash,
        "[" => KeyCode::BracketLeft,
        "]" => KeyCode::BracketRight,
        "\\" => KeyCode::Backslash,
        _ => return None,
    })
}
