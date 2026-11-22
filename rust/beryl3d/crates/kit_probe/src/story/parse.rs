//! The line grammar of stories:
//! - `story <element|container|effect> <width>x<height> <desktop|phone>`
//! - Element: tree lines indented two spaces per level. The flat place `layer` is optional.
//!   A line can end with ` | ` and space-separated `key=value` paint or widget properties.
//! - Container: `container <row|fan> <x>,<y> <width>x<height> gap=<points> max=<count>`,
//!   then one `item <width>x<height> group=<name>` per item.
//! - Effect: `effect <effect-name> <x>,<y> <width>x<height>`.
//!
//! Every line ends in LF. The crate guide gives examples and the property and state vocabulary.
//!
//! ```text
//! story element 360x240 desktop
//! button "Save" #1 12,12 160x48 interactive primary | widget=button
//! ```
//!
//! ```text
//! story container 360x240 desktop
//! container row 12,12 320x160 gap=6 max=40
//! item 40x56 group=first
//! ```
//!
//! ```text
//! story effect 360x240 desktop
//! effect glow 114,60 132x120
//! ```

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use kit_base::data::profiles::SizeClass;
use kit_base::geometry::{Rect, Size};
use kit_frame::frame::{ElementId, Role};

use crate::agent::line::{Line, quoted_name};

use super::style::Property;
use super::{Body, Container, Header, Item, Node, NodeState, Policy, STATES, Story};

const HEADER: &str = "story <element|container|effect> <width>x<height> <desktop|phone>";
const ELEMENT: &str = "role \"name\" #id [layer] x,y widthxheight [states] [ | key=value ...]";
const CONTAINER: &str = "container <row|fan> x,y widthxheight gap=2 max=60";
const ITEM: &str = "item widthxheight group=name (letters, numbers, - or _ in the group)";
const DEPTH: usize = 32;
const MOST_NODES: usize = 512;

/// Where a story could not be read and what that line accepts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    reason: String,
    expected: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            out,
            "line {}: {}. Write {}.",
            self.line, self.reason, self.expected
        )
    }
}

impl std::error::Error for ParseError {}

fn error(line: usize, reason: impl Into<String>, expected: impl Into<String>) -> ParseError {
    ParseError {
        line,
        reason: reason.into(),
        expected: expected.into(),
    }
}

impl FromStr for Story {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut lines = text.split_terminator('\n');
        let first = lines.next().unwrap_or_default();
        let (kind, header) =
            parse_header(first).ok_or_else(|| error(1, "invalid header", HEADER))?;
        let body = match kind {
            "element" => elements(lines)?,
            "container" => container(lines)?,
            "effect" => effect(lines)?,
            _ => return Err(error(1, "unknown story kind", HEADER)),
        };
        let story = Self { header, body };
        let canonical = story.to_string();
        if let Some((index, (_, expected))) = text
            .split_inclusive('\n')
            .zip(canonical.split_inclusive('\n'))
            .enumerate()
            .find(|(_, (given, expected))| given != expected)
        {
            return Err(error(
                index + 1,
                "use the canonical spelling and end the line with LF",
                expected.trim_end(),
            ));
        }
        Ok(story)
    }
}

/// Reads the header shared by toolkit, table and feature stories.
#[must_use]
pub fn parse_header(line: &str) -> Option<(&str, Header)> {
    let mut words = line.split(' ');
    let ("story", kind, window, profile, None) = (
        words.next()?,
        words.next()?,
        words.next()?,
        words.next()?,
        words.next(),
    ) else {
        return None;
    };
    let size_class = SizeClass::from_legacy_layout(profile)?;
    Some((
        kind,
        Header {
            size_class,
            window: size(window)?,
        },
    ))
}

fn elements<'a>(lines: impl Iterator<Item = &'a str>) -> Result<Body, ParseError> {
    let mut nodes: Vec<Node> = Vec::new();
    let mut ids = BTreeSet::new();
    for (line, text) in lines.enumerate().map(|(line, text)| (line + 2, text)) {
        let node = node(line, text)?;
        let depth = node.depth;
        if depth > DEPTH || depth > nodes.last().map_or(0, |parent| parent.depth + 1) {
            return Err(error(
                line,
                "indent skips a parent or is too deep",
                "two spaces for each parent level, up to 32 levels",
            ));
        }
        if !ids.insert(node.id.0) {
            return Err(error(
                line,
                "the id already belongs to another element",
                "a unique #id",
            ));
        }
        if nodes.len() == MOST_NODES {
            return Err(error(
                line,
                "the story has too many elements",
                "at most 512 elements",
            ));
        }
        nodes.push(node);
    }
    if nodes.is_empty() {
        return Err(error(2, "the story has no elements", ELEMENT));
    }
    Ok(Body::Elements(nodes))
}

fn node(line: usize, text: &str) -> Result<Node, ParseError> {
    let (tree, properties) = split_properties(text);
    let parsed = Line::parse(tree).map_err(|why| error(line, why, ELEMENT))?;
    let Line::Element { depth, element } = parsed else {
        return Err(error(line, "expected an element", ELEMENT));
    };
    let role =
        Role::from_word(&element.role).ok_or_else(|| error(line, "unknown role", ELEMENT))?;
    if element
        .place
        .as_deref()
        .is_some_and(|place| place != "layer")
    {
        return Err(error(
            line,
            "toolkit elements live in the flat layer",
            "layer or omit the place",
        ));
    }
    let rect = rect(
        &format!("{},{}", element.x, element.y),
        &format!("{}x{}", element.width, element.height),
    )
    .ok_or_else(|| {
        error(
            line,
            "invalid box",
            "finite coordinates and positive dimensions up to 16384",
        )
    })?;
    let mut seen = BTreeSet::new();
    let states = element.states.iter().map(|word| {
        let state = state(word).ok_or_else(|| error(line, format!("unknown toolkit state {word:?}"), "interactive, hovered, held, selected, disabled, focused, primary or faded(0% to 100%)"))?;
        if !seen.insert(word.split('(').next()) {
            return Err(error(line, "a state is repeated", "each state at most once"));
        }
        Ok(state)
    }).collect::<Result<_, _>>()?;
    let properties = properties.map_or_else(|| Ok(Vec::new()), |text| styles(line, text, role))?;
    if properties
        .iter()
        .any(|property| matches!(property, Property::Level(_)))
        && !properties.contains(&Property::Widget(kit_ui::widgets::Widget::Slider))
    {
        return Err(error(
            line,
            "only a slider has a level",
            "level=0 through level=100 with widget=slider",
        ));
    }
    Ok(Node {
        depth,
        role,
        name: element.name,
        id: ElementId(element.id),
        explicit_layer: element.place.is_some(),
        rect,
        states,
        properties,
    })
}

/// A properties separator in an element's quoted name belongs to that name.
fn split_properties(line: &str) -> (&str, Option<&str>) {
    let mut quoted = false;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if escaped {
            escaped = false;
        } else if character == '\\' && quoted {
            escaped = true;
        } else if character == '"' {
            quoted = !quoted;
        } else if character == ' ' && !quoted && line[index..].starts_with(" | ") {
            return (&line[..index], Some(&line[index + 3..]));
        }
    }
    (line, None)
}

fn state(word: &str) -> Option<NodeState> {
    if word == "interactive" {
        return Some(NodeState::Interactive);
    }
    if let Some(percent) = word
        .strip_prefix("faded(")
        .and_then(|word| word.strip_suffix("%)"))
    {
        return Some(NodeState::Faded(
            number(percent, 0.0).filter(|value| *value <= 100.0)?,
        ));
    }
    STATES
        .iter()
        .find(|(_, name)| *name == word)
        .map(|(bit, _)| NodeState::Bit(*bit))
}

fn styles(line: usize, mut text: &str, role: Role) -> Result<Vec<Property>, ParseError> {
    let mut properties = Vec::new();
    let mut keys = BTreeSet::new();
    loop {
        let wrong = |why| {
            error(
                line,
                why,
                "fill=#rrggbbaa, ink=#rrggbbaa, radius=8, border=1,#rrggbbaa, linear=90,#rrggbbaa,#rrggbbaa, shadow=0,2,6,#rrggbbaa, text=\"words\", type=<title|body>, level=50 or widget=<name> (see the crate guide)",
            )
        };
        let (key, value) = text
            .split_once('=')
            .ok_or_else(|| wrong("missing style property"))?;
        if !keys.insert(key) {
            return Err(wrong("a style property is repeated"));
        }
        let (property, rest) = if key == "text" {
            let (words, rest) = value
                .strip_prefix('"')
                .and_then(quoted_name)
                .ok_or_else(|| wrong("text must be quoted"))?;
            (Property::Text(words), rest)
        } else {
            let end = value.find(' ').unwrap_or(value.len());
            let property = Property::parse(key, &value[..end])
                .ok_or_else(|| wrong("invalid style property or value"))?;
            (property, &value[end..])
        };
        if let Property::Widget(widget) = property
            && widget.role() != role
        {
            return Err(error(
                line,
                "widget does not match the element role",
                format!(
                    "the {} role for {key}={}",
                    widget.role().word(),
                    widget.word()
                ),
            ));
        }
        properties.push(property);
        if rest.is_empty() {
            return Ok(properties);
        }
        text = rest
            .strip_prefix(' ')
            .ok_or_else(|| wrong("properties need a space between them"))?;
    }
}

fn container<'a>(mut lines: impl Iterator<Item = &'a str>) -> Result<Body, ParseError> {
    let mut result = container_header(lines.next().unwrap_or_default())
        .ok_or_else(|| error(2, "invalid container", CONTAINER))?;
    for (index, text) in lines.enumerate() {
        let bad = || error(index + 3, "invalid item", ITEM);
        let mut words = text.split(' ');
        let (Some("item"), Some(dimensions), Some(group), None) =
            (words.next(), words.next(), words.next(), words.next())
        else {
            return Err(bad());
        };
        let group = group
            .strip_prefix("group=")
            .filter(|name| {
                !name.is_empty()
                    && name
                        .bytes()
                        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'-' | b'_'))
            })
            .ok_or_else(bad)?;
        if result.items.len() >= usize::from(result.limit) {
            return Err(error(
                index + 3,
                "the item exceeds max",
                "at most max item lines",
            ));
        }
        result.items.push(Item {
            size: size(dimensions).ok_or_else(bad)?,
            group: group.to_owned(),
        });
    }
    Ok(Body::Container(result))
}

fn container_header(line: &str) -> Option<Container> {
    let mut words = line.split(' ');
    let ("container", policy, corner, size, gap, limit, None) = (
        words.next()?,
        words.next()?,
        words.next()?,
        words.next()?,
        words.next()?,
        words.next()?,
        words.next(),
    ) else {
        return None;
    };
    Some(Container {
        policy: match policy {
            "row" => Policy::Row,
            "fan" => Policy::Fan,
            _ => return None,
        },
        region: rect(corner, size)?,
        gap: number(gap.strip_prefix("gap=")?, 0.0)?,
        limit: limit
            .strip_prefix("max=")?
            .parse::<u16>()
            .ok()
            .filter(|limit| (1..=512).contains(limit))?,
        items: Vec::new(),
    })
}

fn effect<'a>(mut lines: impl Iterator<Item = &'a str>) -> Result<Body, ParseError> {
    let bad = || error(2, "invalid effect", "effect <name> x,y widthxheight");
    let mut words = lines.next().ok_or_else(bad)?.split(' ');
    let (Some("effect"), Some(name), Some(corner), Some(dimensions), None) = (
        words.next(),
        words.next(),
        words.next(),
        words.next(),
        words.next(),
    ) else {
        return Err(bad());
    };
    if name.is_empty() {
        return Err(bad());
    }
    let rect = rect(corner, dimensions).ok_or_else(bad)?;
    if lines.next().is_some() {
        return Err(error(
            3,
            "an effect has one rectangle",
            "the end of the file",
        ));
    }
    Ok(Body::Effect(name.to_owned(), rect))
}

pub(super) fn number(word: &str, min: f32) -> Option<f32> {
    word.parse::<f32>()
        .ok()
        .filter(|value| (min..=16384.0).contains(value))
}

fn size(word: &str) -> Option<Size> {
    let (width, height) = word.split_once('x')?;
    let positive = |word| number(word, 0.0).filter(|value| *value > 0.0);
    Some(Size {
        width: positive(width)?,
        height: positive(height)?,
    })
}

fn rect(corner: &str, dimensions: &str) -> Option<Rect> {
    let (x, y) = corner.split_once(',')?;
    let Size { width, height } = size(dimensions)?;
    Some(Rect {
        x: number(x, -16384.0)?,
        y: number(y, -16384.0)?,
        width,
        height,
    })
}
