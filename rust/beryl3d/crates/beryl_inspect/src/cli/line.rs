//! The grammar of a tree printout's lines after its two header lines (docs/beryl-3d.md, "What
//! is printed"), read back into fields: for `--json`, and for the tests that hold `tree` to the
//! grammar.
//!
//! A line is two spaces of indent per level, then one of:
//! - a group: `game`, `table` or `layer`;
//! - a cut: `... <count> more in <place> (--within <place>)`;
//! - an element: `<role> ["<name>"] #<id> [<place>] <x>,<y> <width>x<height> [<state>...]`,
//!   where a name escapes `"` and `\` with a backslash and a state is a word of the closed list,
//!   `moving`, `pile`, `covered` and `faded` taking a parenthesised value, and `copy` and
//!   `note` a parenthesised name in quotes, escaped as an element's name is.

use beryl_scene::frame::Role;

/// The words a state may be.
const STATES: [&str; 19] = [
    "interactive",
    "tapped",
    "face-down",
    "moving",
    "hovered",
    "held",
    "eligible",
    "selected",
    "target",
    "disabled",
    "focused",
    "phased-out",
    "picked",
    "primary",
    "pile",
    "copy",
    "note",
    "covered",
    "faded",
];

/// One line, read.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    Group {
        depth: usize,
        word: String,
    },
    Cut {
        depth: usize,
        count: usize,
        place: String,
    },
    Element {
        depth: usize,
        element: Element,
    },
}

/// An element's line, read.
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    pub role: String,
    pub name: Option<String>,
    pub id: u32,
    pub place: Option<String>,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub states: Vec<String>,
}

impl Line {
    /// Reads one line.
    ///
    /// # Errors
    ///
    /// When the line does not follow the grammar, saying where.
    pub fn parse(line: &str) -> Result<Self, String> {
        let body = line.trim_start_matches(' ');
        let indent = line.len() - body.len();
        if !indent.is_multiple_of(2) {
            return Err(format!(
                "'{line}': the indent is not a whole number of levels"
            ));
        }
        let depth = indent / 2;
        if matches!(body, "game" | "table" | "layer") {
            return Ok(Self::Group {
                depth,
                word: body.to_owned(),
            });
        }
        if let Some(cut) = body.strip_prefix("... ") {
            return cut_line(cut)
                .map(|(count, place)| Self::Cut {
                    depth,
                    count,
                    place,
                })
                .ok_or_else(|| format!("'{line}' is not a cut line"));
        }
        element(body)
            .map(|element| Self::Element { depth, element })
            .map_err(|why| format!("'{line}': {why}"))
    }
}

fn cut_line(cut: &str) -> Option<(usize, String)> {
    let (count, rest) = cut.split_once(" more in ")?;
    let (place, within) = rest.split_once(' ')?;
    (within == format!("(--within {place})")).then_some(())?;
    Some((count.parse().ok()?, place.to_owned()))
}

fn element(body: &str) -> Result<Element, String> {
    let (role, mut rest) = body.split_once(' ').ok_or("no id")?;
    if Role::from_word(role).is_none() {
        return Err(format!("'{role}' is not a role"));
    }
    let mut name = None;
    if let Some(quoted) = rest.strip_prefix('"') {
        let (text, after) = quoted_name(quoted).ok_or("the name's quote is not closed")?;
        name = Some(text);
        rest = after.strip_prefix(' ').ok_or("nothing follows the name")?;
    }
    let mut words = rest.split(' ').peekable();
    let id = words
        .next()
        .and_then(|word| word.strip_prefix('#'))
        .and_then(|digits| digits.parse().ok())
        .ok_or("no #id")?;
    let mut place = None;
    if let Some(word) = words.peek()
        && word.starts_with(|c: char| c.is_ascii_lowercase())
    {
        place = Some((*word).to_owned());
        words.next();
    }
    let (x, y) = words
        .next()
        .and_then(|word| word.split_once(','))
        .and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?)))
        .ok_or("no x,y corner")?;
    let (width, height) = words
        .next()
        .and_then(|word| word.split_once('x'))
        .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
        .ok_or("no <width>x<height> size")?;
    let states = states(&words.collect::<Vec<_>>().join(" "))?;
    Ok(Element {
        role: role.to_owned(),
        name,
        id,
        place,
        x,
        y,
        width,
        height,
        states,
    })
}

/// The name up to its closing quote, unescaped, and what follows it.
fn quoted_name(text: &str) -> Option<(String, &str)> {
    let mut name = String::new();
    let mut escaped = false;
    for (at, c) in text.char_indices() {
        match c {
            _ if escaped => {
                name.push(c);
                escaped = false;
            }
            '\\' => escaped = true,
            '"' => return Some((name, &text[at + 1..])),
            _ => name.push(c),
        }
    }
    None
}

/// The states, each a word of the closed list, with a parenthesised value where it takes one.
fn states(text: &str) -> Result<Vec<String>, String> {
    let mut states = Vec::new();
    let mut rest = text.trim();
    while !rest.is_empty() {
        let end = rest.find([' ', '(']).unwrap_or(rest.len());
        let word = &rest[..end];
        if !STATES.contains(&word) {
            return Err(format!("'{word}' is not a state"));
        }
        let mut state = word.to_owned();
        rest = &rest[end..];
        if let Some(quoted) = rest.strip_prefix("(\"")
            && matches!(word, "copy" | "note")
        {
            let (text, after) = quoted_name(quoted).ok_or("a state's name is not closed")?;
            let after = after
                .strip_prefix(')')
                .ok_or("a state's name is not closed")?;
            let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
            states.push(format!("{word}(\"{escaped}\")"));
            rest = after.trim_start();
            continue;
        }
        if matches!(word, "copy" | "note") {
            return Err(format!("the state '{word}' takes a name in quotes"));
        }
        if let Some(value) = rest.strip_prefix('(') {
            if !matches!(word, "moving" | "pile" | "covered" | "faded") {
                return Err(format!("the state '{word}' takes no value"));
            }
            let close = value.find(')').ok_or("a state's value is not closed")?;
            let inner = &value[..close];
            if word == "pile" {
                if !inner.parse::<u16>().is_ok_and(|count| count > 1) {
                    return Err(format!(
                        "'{word}({inner})' needs a pile size of two or more"
                    ));
                }
            } else if !inner.ends_with('%') {
                return Err(format!("'{word}({inner})' is not a percentage"));
            }
            state = format!("{word}({inner})");
            rest = &value[close + 1..];
        }
        states.push(state);
        rest = rest.trim_start();
    }
    Ok(states)
}
