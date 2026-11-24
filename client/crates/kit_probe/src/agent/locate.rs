//! Locators: the start of a tree line, which finds one element (docs/beryl-3d.md, "Times,
//! moments, locators and actions").
//!
//! A locator is a role, then optionally a quoted name, then optionally `#` and an id:
//! `button "Auto Pay"`, `card "Island" #8`, `card #8`. Whatever follows on a tree line (the
//! place, the box and the states) is ignored, so a whole tree line is a locator. ` > ` looks
//! inside another element: `region "prompt" > button "Cancel"` finds a button anywhere below
//! that region. A name matches whole and exactly.

use std::fmt;

use super::tree::locator_of;
use kit_frame::frame::{Frame, Labels, Role};
use kit_scene::hit::is_within;

/// How many elements an error lists.
const LISTED: usize = 5;

/// Why a locator found no single element.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocateError {
    /// The text is not a locator.
    Syntax { locator: String, reason: String },
    /// No element matches. `nearest` holds the locators of the elements with the nearest
    /// names.
    NotFound {
        locator: String,
        nearest: Vec<String>,
    },
    /// Several elements match. `matches` holds the first few of their locators, and `count`
    /// how many there are.
    Ambiguous {
        locator: String,
        matches: Vec<String>,
        count: usize,
    },
}

impl fmt::Display for LocateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax { locator, reason } => {
                write!(
                    f,
                    "'{locator}' is not a locator: {reason}. A locator is a role, an optional \
                     quoted name and an optional #id, as in: button \"Auto Pay\", card \"Island\" #8"
                )
            }
            Self::NotFound { locator, nearest } => {
                write!(f, "no element matches '{locator}'.")?;
                if nearest.is_empty() {
                    write!(f, " The frame has no elements.")
                } else {
                    write!(f, " The nearest names are:")?;
                    for line in nearest {
                        write!(f, "\n  {line}")?;
                    }
                    Ok(())
                }
            }
            Self::Ambiguous {
                locator,
                matches,
                count,
            } => {
                write!(f, "{count} elements match '{locator}':")?;
                for line in matches {
                    write!(f, "\n  {line}")?;
                }
                if *count > matches.len() {
                    write!(f, "\n  ... {} more", count - matches.len())?;
                }
                write!(
                    f,
                    "\nNarrow it with an #id, as in '{}', or with '>' to look inside another \
                     element.",
                    matches[0]
                )
            }
        }
    }
}

impl std::error::Error for LocateError {}

/// The index of the one element `locator` names.
///
/// # Errors
///
/// When the text is not a locator, or when it matches no element or more than one.
pub fn locate(frame: &Frame, locator: &str) -> Result<u32, LocateError> {
    let parts = split(locator)
        .into_iter()
        .map(|part| parse(part, frame.labels).map_err(|reason| syntax(locator, reason)))
        .collect::<Result<Vec<_>, _>>()?;
    // The elements the parts so far match, as a chain from the first part.
    let mut found: Vec<u32> = (0..frame.resolved.len() as u32)
        .filter(|&index| parts[0].matches(frame, index))
        .collect();
    for part in &parts[1..] {
        found = (0..frame.resolved.len() as u32)
            .filter(|&index| {
                part.matches(frame, index)
                    && found
                        .iter()
                        .any(|&outer| outer != index && is_within(frame, index, outer))
            })
            .collect();
    }
    match found.as_slice() {
        [index] => Ok(*index),
        [] => Err(LocateError::NotFound {
            locator: locator.to_owned(),
            nearest: nearest(frame, parts.last().expect("a locator has a part")),
        }),
        several => Err(LocateError::Ambiguous {
            locator: locator.to_owned(),
            matches: several
                .iter()
                .take(LISTED)
                .map(|&index| locator_of(frame, index))
                .collect(),
            count: several.len(),
        }),
    }
}

/// One step of a locator: `role "name" #id`.
struct Part {
    role: Role,
    name: Option<String>,
    id: Option<u32>,
}

impl Part {
    fn matches(&self, frame: &Frame, index: u32) -> bool {
        let element = &frame.resolved[index as usize];
        element.role == self.role
            && self
                .name
                .as_deref()
                .is_none_or(|name| frame.name(element.name) == name)
            && self.id.is_none_or(|id| element.id.0 == id)
    }
}

fn syntax(locator: &str, reason: String) -> LocateError {
    LocateError::Syntax {
        locator: locator.to_owned(),
        reason,
    }
}

/// The locator's steps: the text split at each `>` outside quotes.
fn split(locator: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut start, mut quoted, mut escaped) = (0, false, false);
    for (at, char) in locator.char_indices() {
        match char {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            '>' if !quoted => {
                parts.push(locator[start..at].trim());
                start = at + 1;
            }
            _ => {}
        }
    }
    parts.push(locator[start..].trim());
    parts
}

fn parse(text: &str, labels: &Labels) -> Result<Part, String> {
    let (word, mut rest) = text.split_once(' ').unwrap_or((text, ""));
    let role = labels.role(word).ok_or_else(|| {
        let roles: Vec<&str> = labels.roles().map(Role::word).collect();
        format!("'{word}' is not a role; the roles are {}", roles.join(", "))
    })?;
    rest = rest.trim_start();
    let mut name = None;
    if let Some(quoted) = rest.strip_prefix('"') {
        let mut text = String::new();
        let mut chars = quoted.char_indices();
        let end = loop {
            match chars.next() {
                None => return Err("the name has no closing quote".to_owned()),
                Some((_, '\\')) => match chars.next() {
                    Some((_, char)) => text.push(char),
                    None => return Err("the name ends in a backslash".to_owned()),
                },
                Some((at, '"')) => break at,
                Some((_, char)) => text.push(char),
            }
        };
        name = Some(text);
        rest = quoted[end + 1..].trim_start();
    }
    let mut id = None;
    // A tree line goes on after its id with the place, box and states, which are ignored.
    // Anything else after the name is a mistake, such as a name without quotes.
    if !rest.is_empty() && !rest.starts_with('#') {
        return Err(format!(
            "'{rest}' should be a quoted name or an #id; a name is written in double quotes"
        ));
    }
    if let Some(number) = rest.strip_prefix('#') {
        let digits = number.split(' ').next().unwrap_or("");
        id = Some(
            digits
                .parse()
                .map_err(|_| format!("'#{digits}' is not an id"))?,
        );
    }
    Ok(Part { role, name, id })
}

/// The locators of the elements whose names are nearest to `part`'s, nearest first, those of
/// its role before others.
fn nearest(frame: &Frame, part: &Part) -> Vec<String> {
    let wanted = part.name.as_deref().unwrap_or("");
    let mut scored: Vec<(bool, usize, u32)> = (0..frame.resolved.len() as u32)
        .map(|index| {
            let element = &frame.resolved[index as usize];
            (
                element.role != part.role,
                distance(wanted, frame.name(element.name)),
                index,
            )
        })
        .collect();
    scored.sort_unstable();
    let mut out: Vec<String> = Vec::new();
    for (_, _, index) in scored {
        let line = locator_of(frame, index);
        if !out.contains(&line) {
            out.push(line);
        }
        if out.len() == LISTED {
            break;
        }
    }
    out
}

/// The edit distance between two names, counting characters case-insensitively.
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.to_lowercase().chars().collect();
    let b: Vec<char> = b.to_lowercase().chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, &first) in a.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, &second) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (above + 1)
                .min(row[j] + 1)
                .min(diagonal + usize::from(first != second));
            diagonal = above;
        }
    }
    row[b.len()]
}
