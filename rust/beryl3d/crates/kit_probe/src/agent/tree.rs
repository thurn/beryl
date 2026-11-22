//! The tree printout: a frame's resolved elements as indented text (docs/beryl-3d.md, "What is
//! printed"). Every line is `role "name" #id place x,y wxh states`, and its start up to the id
//! is a locator for that element.

use std::fmt::Write;

use super::fault::rect_text;
use super::locate::{LocateError, locate};
use kit_frame::frame::{ElementId, Frame, Resolved, Role, StateBits};
use kit_scene::hit::covered;
use kit_scene::{Node, Space, Tree};

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
    tree_with_places(frame, options, |_| false)
}

/// Prints the tree, also recognizing valid place labels that have no elements in this frame.
///
/// # Errors
///
/// When `options.within` is neither a recognized place nor a locator of exactly one element.
pub fn tree_with_places(
    frame: &Frame,
    options: &TreeOptions,
    known_place: impl Fn(&str) -> bool,
) -> Result<String, LocateError> {
    let tree = Tree::from_frame(frame);
    let children = children(&tree);
    let within = match options.within.as_deref() {
        None => Within::All,
        Some(text)
            if !text.is_empty()
                && (known_place(text)
                    || frame
                        .resolved
                        .iter()
                        .any(|element| element.place.label() == text)) =>
        {
            Within::Place(text)
        }
        Some(text) => Within::Element(locate(frame, text)?),
    };
    let whole = options.full || options.role.is_some() || !matches!(within, Within::All);
    let mut out = String::from("game\n");
    for (header, layer) in [("table", false), ("layer", true)] {
        let roots: Vec<u32> = children[ROOT]
            .iter()
            .copied()
            .filter(|&index| {
                matches!(
                    tree.node(index).expect("a registered node").space,
                    Space::Screen { .. }
                ) == layer
            })
            .collect();
        let mut lines = String::new();
        let printer = Printer {
            frame,
            tree: &tree,
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
    locator(element.id, element.role, frame.name(element.name))
}

fn locator(id: ElementId, role: Role, name: &str) -> String {
    let mut out = String::from(role.word());
    if !name.is_empty() {
        let _ = write!(
            out,
            " \"{}\"",
            name.replace('\\', "\\\\").replace('"', "\\\"")
        );
    }
    let _ = write!(out, " #{}", id.0);
    out
}

/// Which part of the tree is printed.
#[derive(Clone, Copy)]
enum Within<'a> {
    All,
    Place(&'a str),
    Element(u32),
}

/// The index in `children` that holds the root elements.
const ROOT: usize = 0;

/// Each element's children in tree order: entry 0 holds the roots, entry `i + 1` the children
/// of element `i`.
fn children(tree: &Tree<&Frame>) -> Vec<Vec<u32>> {
    let mut children = vec![Vec::new(); tree.nodes().len() + 1];
    for (index, node) in tree.nodes().enumerate() {
        let parent = node.parent.map_or(ROOT, |index| index as usize + 1);
        children[parent].push(index as u32);
    }
    children
}

struct Printer<'a> {
    frame: &'a Frame,
    tree: &'a Tree<&'a Frame>,
    children: &'a [Vec<u32>],
    options: &'a TreeOptions,
    within: Within<'a>,
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
            let node = self.node(index);
            let place = self.frame.resolved[index as usize].place;
            if node.role != Role::Card || matches!(node.space, Space::Screen { .. }) {
                self.element(index, indent, inside, out);
                continue;
            }
            // Every card of this place among the siblings, printed together.
            let group: Vec<usize> = (at..siblings.len())
                .filter(|&other| {
                    let candidate = siblings[other];
                    self.node(candidate).role == Role::Card
                        && self.frame.resolved[candidate as usize].place.label() == place.label()
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
                let word = place.label();
                let _ = writeln!(
                    out,
                    "{:indent$}... {} more in {word} (--within {word})",
                    "",
                    group.len() - shown,
                );
            }
        }
    }

    fn node(&self, index: u32) -> Node<'_> {
        self.tree.node(index).expect("a registered node")
    }

    fn element(&self, index: u32, indent: usize, inside: bool, out: &mut String) {
        let node = self.node(index);
        let matched = inside
            || match self.within {
                Within::All => true,
                Within::Place(place) => self.frame.resolved[index as usize].place.label() == place,
                Within::Element(target) => target == index,
            };
        let shown = matched && self.options.role.is_none_or(|role| role == node.role);
        let children = &self.children[index as usize + 1];
        if shown {
            let _ = writeln!(out, "{:indent$}{}", "", line_for(self.frame, index, &node));
            self.siblings(children, indent + 2, matched, out);
        } else {
            self.siblings(children, indent, matched, out);
        }
    }
}

/// One element's tree line, without its indent.
#[must_use]
pub fn line(frame: &Frame, index: u32) -> String {
    let node = Tree::from_frame(frame)
        .node(index)
        .expect("a registered node");
    line_for(frame, index, &node)
}

fn line_for(frame: &Frame, index: u32, node: &Node<'_>) -> String {
    let element = &frame.resolved[index as usize];
    let mut out = locator(node.id, node.role, &node.name);
    let place = element.place.label();
    if !place.is_empty() {
        let _ = write!(out, " {place}");
    }
    let _ = write!(out, " {}", rect_text(node.space.screen_bounds()));
    states(frame, index, element, &mut out);
    out
}

/// Appends the element's states, in the order of the doc's closed list.
fn states(frame: &Frame, index: u32, element: &Resolved, out: &mut String) {
    if element.interactive {
        out.push_str(" interactive");
    }
    for word in frame.labels.states(element.state) {
        out.push(' ');
        out.push_str(word);
    }
    if element.count > 1
        && let Some(word) = frame.labels.count
    {
        let _ = write!(out, " {word}({})", element.count);
    }
    for (label, name) in frame
        .labels
        .annotations
        .into_iter()
        .zip(element.annotations)
    {
        let text = frame.name(name);
        if !text.is_empty()
            && let Some(word) = label
        {
            let _ = write!(
                out,
                " {word}(\"{}\")",
                text.replace('\\', "\\\\").replace('"', "\\\"")
            );
        }
    }
    // An element the pointer goes through is never reached by a press, so no share of it is
    // covered in the sense the word means.
    if element.role != Role::Arrow && !element.state.has(StateBits::PASSES) {
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
