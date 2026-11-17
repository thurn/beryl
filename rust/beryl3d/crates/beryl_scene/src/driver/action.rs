//! Actions: one line each, the same in a test, a drive script and the running app's socket
//! (docs/beryl-3d.md, "Times, moments, locators and actions").

use glam::Vec2;

use super::moment::{Duration, ParseError};
use crate::input::{KeyCode, Modifiers};

/// One action. Wherever an action takes a locator, it also takes a point of the window,
/// `<x>,<y>` in logical points (`point`), so that a drag can start on the empty table, where no
/// element is.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// `press <locator>`
    Press(String),
    /// `hover <locator>`
    Hover(String),
    /// `menu <locator>`: a right click on the element, which opens a context menu where it
    /// has one, as on the manual table.
    Menu(String),
    /// `drag <locator> to <locator>`
    Drag(String, String),
    /// `hold <locator> over <locator>`: a drag that is not let go, which presses on the first
    /// element and moves to the second as `drag` does and keeps the button down there, so that
    /// what a drag shows while it is over something can be seen.
    Hold(String, String),
    /// `type <locator> "<text>"`
    Type(String, String),
    /// `key <name>`, or `key <modifiers>+<name>` as in `key Cmd+B`
    Key(KeyChord),
    /// `move <x>,<y>`
    Move(Vec2),
    /// `wheel <locator> <x>,<y>`: turns the mouse wheel over the element by that many logical
    /// points, as a host gives it. Positive moves the content right and down, as a wheel turned
    /// up does, so `wheel list "Opponent" 0,-400` scrolls a list toward its end.
    Wheel(String, Vec2),
    /// `step <duration>`
    Step(Duration),
    /// `still`
    Still,
}

/// A key with the modifiers held while it is pressed, as a `key` action names them. `Cmd` is
/// the Command key on the Mac and Control elsewhere, as in a shortcut's binding
/// (`element::Chord`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "one flag per modifier a chord may name"
)]
pub struct KeyChord {
    pub code: KeyCode,
    pub cmd: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl KeyChord {
    /// `code` with no modifier held.
    #[must_use]
    pub fn plain(code: KeyCode) -> Self {
        Self {
            code,
            cmd: false,
            ctrl: false,
            alt: false,
            shift: false,
        }
    }

    /// The chord `name` names: a key's name, after any of `Cmd+`, `Ctrl+`, `Alt+` (or
    /// `Option+`) and `Shift+`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        let mut parts: Vec<&str> = name.split('+').collect();
        let mut chord = Self::plain(key_code(parts.pop()?)?);
        for modifier in parts {
            let flag = match modifier {
                "Cmd" => &mut chord.cmd,
                "Ctrl" => &mut chord.ctrl,
                "Alt" | "Option" => &mut chord.alt,
                "Shift" => &mut chord.shift,
                _ => return None,
            };
            *flag = true;
        }
        Some(chord)
    }

    /// The modifier keys held, on the Mac if `mac`.
    #[must_use]
    pub fn modifiers(self, mac: bool) -> Modifiers {
        Modifiers {
            shift: self.shift,
            control: self.ctrl || (self.cmd && !mac),
            alt: self.alt,
            command: self.cmd && mac,
        }
    }
}

impl std::str::FromStr for Action {
    type Err = ParseError;

    fn from_str(line: &str) -> Result<Self, ParseError> {
        let line = line.trim();
        let actions = "The actions are press <locator>, hover <locator>, menu <locator>, drag <locator> to \
                       <locator>, hold <locator> over <locator>, type <locator> \"<text>\", key \
                       <name>, move <x>,<y>, wheel <locator> <x>,<y>, step <duration> and still. A \
                       point <x>,<y> may stand in place of a locator";
        let expected =
            |why: &str| ParseError(format!("'{line}' is not an action: {why}. {actions}"));
        let (verb, rest) = line.split_once(' ').unwrap_or((line, ""));
        let rest = rest.trim();
        let needs = |what: &str| {
            if rest.is_empty() {
                Err(expected(&format!("{verb} needs {what}")))
            } else {
                Ok(rest.to_owned())
            }
        };
        match verb {
            "press" => needs("a locator").map(Self::Press),
            "hover" => needs("a locator").map(Self::Hover),
            "menu" => needs("a locator").map(Self::Menu),
            "drag" => {
                let at = outside_quotes(rest, " to ")
                    .ok_or_else(|| expected("drag needs <locator> to <locator>"))?;
                Ok(Self::Drag(
                    rest[..at].trim().to_owned(),
                    rest[at + 4..].trim().to_owned(),
                ))
            }
            "hold" => {
                let at = outside_quotes(rest, " over ")
                    .ok_or_else(|| expected("hold needs <locator> over <locator>"))?;
                Ok(Self::Hold(
                    rest[..at].trim().to_owned(),
                    rest[at + 6..].trim().to_owned(),
                ))
            }
            "type" => {
                let (at, text) = last_quoted(rest)
                    .filter(|&(at, _)| at > 0)
                    .ok_or_else(|| expected("type needs a locator and then the text in quotes"))?;
                Ok(Self::Type(rest[..at].trim().to_owned(), text))
            }
            "key" => KeyChord::parse(rest).map(Self::Key).ok_or_else(|| {
                expected(&format!(
                    "'{rest}' is not a key's name, with Cmd+, Ctrl+, Alt+ or Shift+ before it"
                ))
            }),
            "move" => point(rest)
                .map(Self::Move)
                .ok_or_else(|| expected("move needs a point, as in move 400,300")),
            "wheel" => rest
                .rsplit_once(' ')
                .and_then(|(locator, by)| Some((locator.trim(), point(by)?)))
                .filter(|(locator, _)| !locator.is_empty())
                .map(|(locator, by)| Self::Wheel(locator.to_owned(), by))
                .ok_or_else(|| {
                    expected(
                        "wheel needs a locator and then a distance, as in \
                         wheel list \"Opponent\" 0,-400",
                    )
                }),
            "step" => rest
                .parse()
                .map(Self::Step)
                .map_err(|ParseError(why)| expected(&why)),
            "still" if rest.is_empty() => Ok(Self::Still),
            "still" => Err(expected("still takes nothing after it")),
            _ => Err(ParseError(format!(
                "'{line}' is not an action: it does not start with one. {actions}"
            ))),
        }
    }
}

/// The point `<x>,<y>` that `text` writes, in logical points, if it writes one rather than a
/// locator.
#[must_use]
pub fn point(text: &str) -> Option<Vec2> {
    let (x, y) = text.split_once(',')?;
    Some(Vec2::new(x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// The quoted string that ends `text`, unescaped, and where its opening quote is.
fn last_quoted(text: &str) -> Option<(usize, String)> {
    let (mut quoted, mut escaped) = (false, false);
    let (mut start, mut body) = (0, String::new());
    let mut last = None;
    for (at, char) in text.char_indices() {
        match char {
            _ if escaped => {
                escaped = false;
                body.push(char);
            }
            '\\' if quoted => escaped = true,
            '"' if quoted => {
                quoted = false;
                last = (at + 1 == text.len()).then(|| (start, std::mem::take(&mut body)));
            }
            '"' => {
                quoted = true;
                start = at;
                body.clear();
            }
            _ if quoted => body.push(char),
            _ => {}
        }
    }
    last
}

/// Where `needle` first appears in `text` outside double quotes.
fn outside_quotes(text: &str, needle: &str) -> Option<usize> {
    let (mut quoted, mut escaped) = (false, false);
    for (at, char) in text.char_indices() {
        match char {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            _ if !quoted && text[at..].starts_with(needle) => return Some(at),
            _ => {}
        }
    }
    None
}

/// The key a name means: a letter, a digit, or `Enter`, `Escape`, `Space`, `Tab`,
/// `Backspace`, `Delete`, `ArrowUp`, `ArrowDown`, `ArrowLeft`, `ArrowRight`, `Home`, `End` or `Back`, in any
/// case.
#[must_use]
pub fn key_code(name: &str) -> Option<KeyCode> {
    const LETTERS: [KeyCode; 26] = [
        KeyCode::A,
        KeyCode::B,
        KeyCode::C,
        KeyCode::D,
        KeyCode::E,
        KeyCode::F,
        KeyCode::G,
        KeyCode::H,
        KeyCode::I,
        KeyCode::J,
        KeyCode::K,
        KeyCode::L,
        KeyCode::M,
        KeyCode::N,
        KeyCode::O,
        KeyCode::P,
        KeyCode::Q,
        KeyCode::R,
        KeyCode::S,
        KeyCode::T,
        KeyCode::U,
        KeyCode::V,
        KeyCode::W,
        KeyCode::X,
        KeyCode::Y,
        KeyCode::Z,
    ];
    const DIGITS: [KeyCode; 10] = [
        KeyCode::Digit0,
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    let mut chars = name.chars();
    if let (Some(char), None) = (chars.next(), chars.next()) {
        if char.is_ascii_alphabetic() {
            return Some(LETTERS[usize::from(char.to_ascii_lowercase() as u8 - b'a')]);
        }
        if char.is_ascii_digit() {
            return Some(DIGITS[usize::from(char as u8 - b'0')]);
        }
    }
    let named = [
        ("enter", KeyCode::Enter),
        ("escape", KeyCode::Escape),
        ("space", KeyCode::Space),
        ("tab", KeyCode::Tab),
        ("backspace", KeyCode::Backspace),
        ("delete", KeyCode::Delete),
        ("arrowup", KeyCode::ArrowUp),
        ("arrowdown", KeyCode::ArrowDown),
        ("arrowleft", KeyCode::ArrowLeft),
        ("arrowright", KeyCode::ArrowRight),
        ("home", KeyCode::Home),
        ("end", KeyCode::End),
        ("back", KeyCode::Back),
        ("f1", KeyCode::F1),
        ("f2", KeyCode::F2),
        ("f3", KeyCode::F3),
        ("f4", KeyCode::F4),
        ("f5", KeyCode::F5),
        ("f6", KeyCode::F6),
        ("f7", KeyCode::F7),
        ("f8", KeyCode::F8),
        ("f9", KeyCode::F9),
        ("f10", KeyCode::F10),
        ("f11", KeyCode::F11),
        ("f12", KeyCode::F12),
        ("minus", KeyCode::Minus),
        ("equal", KeyCode::Equal),
        ("comma", KeyCode::Comma),
        ("period", KeyCode::Period),
        ("slash", KeyCode::Slash),
        ("bracketleft", KeyCode::BracketLeft),
        ("bracketright", KeyCode::BracketRight),
        ("backslash", KeyCode::Backslash),
    ];
    let lower = name.to_ascii_lowercase();
    named
        .into_iter()
        .find(|(word, _)| *word == lower)
        .map(|(_, code)| code)
}
