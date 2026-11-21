//! A toolkit scene authored as text, played with no client or game state.

mod parse;
mod player;
mod style;

pub use parse::ParseError;
pub use player::Player;

use std::fmt;

use kit_base::geometry::{Rect, Size};
use kit_frame::frame::{ElementId, Role, StateBits};
use kit_fx::effects::Effect;

use crate::agent::Layout;
use style::Property;

/// The authored layout and logical window of a toolkit story.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Header {
    pub layout: Layout,
    pub window: Size,
}

/// A parsed toolkit scene. Its display form is the canonical story file, including the final
/// newline. See the crate guide for examples.
#[derive(Clone, Debug, PartialEq)]
pub struct Story {
    pub header: Header,
    body: Body,
}

impl Story {
    /// A still story has one instant. An effect has a timed preview.
    #[must_use]
    pub fn duration(&self) -> u64 {
        match self.body {
            Body::Effect(..) => kit_fx::effects::preview::DURATION,
            _ => 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Body {
    Elements(Vec<Node>),
    Container(Container),
    Effect(Effect, Rect),
}

#[derive(Clone, Debug, PartialEq)]
struct Node {
    depth: usize,
    role: Role,
    name: Option<String>,
    id: ElementId,
    explicit_layer: bool,
    rect: Rect,
    states: Vec<NodeState>,
    properties: Vec<Property>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum NodeState {
    Interactive,
    Bit(u32),
    Faded(f32),
}

const STATES: [(u32, &str); 6] = [
    (StateBits::HOVERED, "hovered"),
    (StateBits::HELD, "held"),
    (StateBits::SELECTED, "selected"),
    (StateBits::DISABLED, "disabled"),
    (StateBits::FOCUSED, "focused"),
    (StateBits::PRIMARY, "primary"),
];

#[derive(Clone, Debug, PartialEq)]
struct Container {
    policy: Policy,
    region: Rect,
    gap: f32,
    limit: u16,
    items: Vec<Item>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Policy {
    Row,
    Fan,
}

impl Policy {
    const fn word(self) -> &'static str {
        match self {
            Self::Row => "row",
            Self::Fan => "fan",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Item {
    size: Size,
    group: String,
}

impl fmt::Display for Story {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self.body {
            Body::Elements(_) => "element",
            Body::Container(_) => "container",
            Body::Effect(..) => "effect",
        };
        let Size { width, height } = self.header.window;
        let profile = match self.header.layout {
            Layout::Desktop => "desktop",
            Layout::Phone => "phone",
        };
        writeln!(out, "story {kind} {width}x{height} {profile}")?;
        match &self.body {
            Body::Elements(nodes) => {
                for node in nodes {
                    writeln!(out, "{node}")?;
                }
            }
            Body::Container(container) => {
                writeln!(
                    out,
                    "container {} {} gap={} max={}",
                    container.policy.word(),
                    BoxText(container.region),
                    container.gap,
                    container.limit
                )?;
                for item in &container.items {
                    let Size { width, height } = item.size;
                    writeln!(out, "item {width}x{height} group={}", item.group)?;
                }
            }
            Body::Effect(effect, rect) => {
                writeln!(out, "effect {} {}", effect.name(), BoxText(*rect))?;
            }
        }
        Ok(())
    }
}

struct BoxText(Rect);

impl fmt::Display for BoxText {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Rect {
            x,
            y,
            width,
            height,
        } = self.0;
        write!(out, "{x},{y} {width}x{height}")
    }
}

impl fmt::Display for Node {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            out,
            "{:indent$}{}",
            "",
            self.role.word(),
            indent = self.depth * 2
        )?;
        if let Some(name) = &self.name {
            write!(
                out,
                " \"{}\"",
                name.replace('\\', "\\\\").replace('"', "\\\"")
            )?;
        }
        write!(out, " #{}", self.id.0)?;
        if self.explicit_layer {
            out.write_str(" layer")?;
        }
        write!(out, " {}", BoxText(self.rect))?;
        for state in &self.states {
            match state {
                NodeState::Interactive => out.write_str(" interactive")?,
                NodeState::Faded(percent) => write!(out, " faded({percent}%)")?,
                NodeState::Bit(bit) => {
                    if let Some((_, name)) = STATES.iter().find(|(known, _)| known == bit) {
                        write!(out, " {name}")?;
                    }
                }
            }
        }
        for (index, property) in self.properties.iter().enumerate() {
            out.write_str(if index == 0 { " | " } else { " " })?;
            write!(out, "{property}")?;
        }
        Ok(())
    }
}
