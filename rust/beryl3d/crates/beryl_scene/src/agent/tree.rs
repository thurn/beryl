//! The tree printout: a frame's resolved elements as indented text (docs/beryl-3d.md, "What is
//! printed"). Every line is `role "name" #id place x,y wxh states`, and its start up to the id
//! is a locator for that element.

use std::fmt::Write;

use beryl_protocol::protocol::{RowName, Side, ZoneName};

use super::fault::rect_text;
use super::hit::{bounds, covered};
use super::locate::{LocateError, locate};
use crate::frame::{Frame, Place, Resolved, Role, StateBits};

/// A place holding more cards than this prints only its first `SHOWN` and a count.
const MOST: usize = 12;
const SHOWN: usize = 3;

/// What `tree` prints.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TreeOptions {
    /// Only this part, whole: a place (`battlefield.yours.lands`) or a locator.
    pub within: Option<String>,
    /// Only elements of this role, whole.
    pub role: Option<Role>,
    /// Every element, with no place cut short.
    pub full: bool,
}

/// The frame's resolved elements, one line each, indented by parent under `game`, then `table`
/// for elements on the table and `layer` for the flat layer. The two header lines that name
/// the command and the moment are the caller's.
///
/// # Errors
///
/// When `options.within` is neither a place nor a locator of exactly one element.
pub fn tree(frame: &Frame, options: &TreeOptions) -> Result<String, LocateError> {
    let children = children(frame);
    let within = match options.within.as_deref() {
        None => Within::All,
        Some(text) => match place_from_word(text) {
            Some(place) => Within::Place(place),
            None => Within::Element(locate(frame, text)?),
        },
    };
    let whole = options.full || options.role.is_some() || !matches!(within, Within::All);
    let mut out = String::from("game\n");
    for (header, layer) in [("table", false), ("layer", true)] {
        let roots: Vec<u32> = children[ROOT]
            .iter()
            .copied()
            .filter(|&index| (frame.resolved[index as usize].place == Place::Layer) == layer)
            .collect();
        let mut lines = String::new();
        let printer = Printer {
            frame,
            children: &children,
            options,
            within,
            whole,
        };
        printer.siblings(&roots, 4, false, &mut lines);
        if !lines.is_empty() {
            let _ = writeln!(out, "  {header}");
            out.push_str(&lines);
        }
    }
    Ok(out)
}

/// The start of an element's tree line, which is a locator for it: role, quoted name and id.
#[must_use]
pub fn locator_of(frame: &Frame, index: u32) -> String {
    let element = &frame.resolved[index as usize];
    let name = frame.name(element.name);
    let mut out = String::from(element.role.word());
    if !name.is_empty() {
        let _ = write!(
            out,
            " \"{}\"",
            name.replace('\\', "\\\\").replace('"', "\\\"")
        );
    }
    let _ = write!(out, " #{}", element.id.0);
    out
}

/// The word the tree uses for a place: the zone, then whose side unless it is the stack, then
/// the battlefield row, joined by dots (`battlefield.yours.lands`). Empty for the flat layer.
#[must_use]
pub fn place_word(place: Place) -> String {
    let Place::Card { zone, side, row } = place else {
        return String::new();
    };
    let mut out = String::from(zone_word(zone));
    if zone != ZoneName::Stack {
        out.push('.');
        out.push_str(side_word(side));
    }
    if let Some(row) = row {
        out.push('.');
        out.push_str(row_word(row));
    }
    out
}

/// The place `word` names, as `place_word` writes it.
#[must_use]
pub fn place_from_word(word: &str) -> Option<Place> {
    let mut parts = word.split('.');
    let first = parts.next()?;
    let zone = ZONES.into_iter().find(|&zone| zone_word(zone) == first)?;
    let side = if zone == ZoneName::Stack {
        Side::You
    } else {
        let part = parts.next()?;
        [Side::You, Side::Opponent]
            .into_iter()
            .find(|&side| side_word(side) == part)?
    };
    let row = match parts.next() {
        None => None,
        Some(part) => Some(
            [RowName::Creatures, RowName::Others, RowName::Lands]
                .into_iter()
                .find(|&row| row_word(row) == part)?,
        ),
    };
    if parts.next().is_some() {
        return None;
    }
    Some(Place::Card { zone, side, row })
}

const ZONES: [ZoneName; 8] = [
    ZoneName::Library,
    ZoneName::Hand,
    ZoneName::Battlefield,
    ZoneName::Graveyard,
    ZoneName::Exile,
    ZoneName::Stack,
    ZoneName::Nowhere,
    ZoneName::Sideboard,
];

fn zone_word(zone: ZoneName) -> &'static str {
    match zone {
        ZoneName::Library => "library",
        ZoneName::Hand => "hand",
        ZoneName::Battlefield => "battlefield",
        ZoneName::Graveyard => "graveyard",
        ZoneName::Exile => "exile",
        ZoneName::Stack => "stack",
        ZoneName::Nowhere => "nowhere",
        ZoneName::Sideboard => "sideboard",
    }
}

fn side_word(side: Side) -> &'static str {
    match side {
        Side::You => "yours",
        Side::Opponent => "theirs",
    }
}

fn row_word(row: RowName) -> &'static str {
    match row {
        RowName::Creatures => "creatures",
        RowName::Others => "others",
        RowName::Lands => "lands",
    }
}

/// Whether two places are the same for the tree: the stack is one place whoever's spell it is.
fn same_place(a: Place, b: Place) -> bool {
    match (a, b) {
        (
            Place::Card {
                zone: ZoneName::Stack,
                row: first,
                ..
            },
            Place::Card {
                zone: ZoneName::Stack,
                row: second,
                ..
            },
        ) => first == second,
        _ => a == b,
    }
}

/// Which part of the tree is printed.
#[derive(Clone, Copy)]
enum Within {
    All,
    Place(Place),
    Element(u32),
}

/// The index in `children` that holds the root elements.
const ROOT: usize = 0;

/// Each element's children in tree order: entry 0 holds the roots, entry `i + 1` the children
/// of element `i`.
fn children(frame: &Frame) -> Vec<Vec<u32>> {
    let mut children = vec![Vec::new(); frame.resolved.len() + 1];
    for (index, element) in frame.resolved.iter().enumerate() {
        let parent = if element.parent == u32::MAX {
            ROOT
        } else {
            element.parent as usize + 1
        };
        children[parent].push(index as u32);
    }
    children
}

struct Printer<'a> {
    frame: &'a Frame,
    children: &'a [Vec<u32>],
    options: &'a TreeOptions,
    within: Within,
    /// Whether no place is cut short.
    whole: bool,
}

impl Printer<'_> {
    /// Prints `siblings` at `indent`, cutting each place of more than `MOST` cards short.
    /// `inside` is whether an ancestor already matched `within`.
    fn siblings(&self, siblings: &[u32], indent: usize, inside: bool, out: &mut String) {
        let mut done = vec![false; siblings.len()];
        for (at, &index) in siblings.iter().enumerate() {
            if done[at] {
                continue;
            }
            let element = &self.frame.resolved[index as usize];
            if element.role != Role::Card || element.place == Place::Layer {
                self.element(index, indent, inside, out);
                continue;
            }
            // Every card of this place among the siblings, printed together.
            let group: Vec<usize> = (at..siblings.len())
                .filter(|&other| {
                    let candidate = &self.frame.resolved[siblings[other] as usize];
                    candidate.role == Role::Card && same_place(candidate.place, element.place)
                })
                .collect();
            for &other in &group {
                done[other] = true;
            }
            let cut = !self.whole && group.len() > MOST;
            let shown = if cut { SHOWN } else { group.len() };
            for &other in &group[..shown] {
                self.element(siblings[other], indent, inside, out);
            }
            if cut {
                let word = place_word(element.place);
                let _ = writeln!(
                    out,
                    "{:indent$}... {} more in {word} (--within {word})",
                    "",
                    group.len() - shown,
                );
            }
        }
    }

    fn element(&self, index: u32, indent: usize, inside: bool, out: &mut String) {
        let element = &self.frame.resolved[index as usize];
        let matched = inside
            || match self.within {
                Within::All => true,
                Within::Place(place) => same_place(element.place, place),
                Within::Element(target) => target == index,
            };
        let shown = matched && self.options.role.is_none_or(|role| role == element.role);
        let children = &self.children[index as usize + 1];
        if shown {
            let _ = writeln!(out, "{:indent$}{}", "", line(self.frame, index));
            self.siblings(children, indent + 2, matched, out);
        } else {
            self.siblings(children, indent, matched, out);
        }
    }
}

/// One element's tree line, without its indent.
#[must_use]
pub fn line(frame: &Frame, index: u32) -> String {
    let element = &frame.resolved[index as usize];
    let mut out = locator_of(frame, index);
    let place = place_word(element.place);
    if !place.is_empty() {
        let _ = write!(out, " {place}");
    }
    let _ = write!(out, " {}", rect_text(bounds(element)));
    states(frame, index, element, &mut out);
    out
}

/// Appends the element's states, in the order of the doc's closed list.
fn states(frame: &Frame, index: u32, element: &Resolved, out: &mut String) {
    if element.interactive {
        out.push_str(" interactive");
    }
    for (bit, word) in [
        (StateBits::TAPPED, "tapped"),
        (StateBits::FACE_DOWN, "face-down"),
        (StateBits::MOVING, "moving"),
        (StateBits::HOVERED, "hovered"),
        (StateBits::HELD, "held"),
        (StateBits::ELIGIBLE, "eligible"),
        (StateBits::SELECTED, "selected"),
        (StateBits::TARGET, "target"),
        (StateBits::DISABLED, "disabled"),
        (StateBits::FOCUSED, "focused"),
    ] {
        if element.state.has(bit) {
            out.push(' ');
            out.push_str(word);
        }
    }
    if element.role != Role::Arrow {
        let percent = (covered(frame, index) * 100.0).round();
        if percent >= 1.0 {
            let _ = write!(out, " covered({percent}%)");
        }
    }
    let faded = ((1.0 - element.opacity) * 100.0).round();
    if faded >= 1.0 {
        let _ = write!(out, " faded({faded}%)");
    }
}
