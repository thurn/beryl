//! The flat layer's retained tree (docs/beryl-3d.md, "Elements", "Sizes" and "Text"). The
//! client hands it the elements a screen function returns, and it matches them against the
//! tree it keeps: a node equal to the one before it keeps its layout and its laid-out text.
//! Layout is flexbox, computed by taffy, and text is shaped and broken by parley through
//! taffy's measure function. All of that happens in `Client::handle`; writing a frame only
//! reads the numbers kept here. Beside the screen it keeps floats, small elements placed at a
//! point of the window (`float`).

mod float;
mod write;

use taffy::{
    AlignContent, AlignItems, AvailableSpace, Dimension, Display, FlexDirection, LengthPercentage,
    NodeId, TaffyTree, compute_leaf_layout,
};

pub use float::FloatLook;

use float::{Float, Prepared};

use crate::element::{Align, Direction, Element, Justify, Press, Style};
use crate::frame::{ElementId, Role};
use crate::input::{Rect, Size};
use crate::table::Window;
use crate::text::{Block, Glyphs, Text, TypeStyle};

/// Where the desktop layout anchors a screen: its bottom right corner this far in from the
/// safe area's, where the web interface's action dock stands (`.dock` in
/// `ui/src/screens/styles/controls.css`: `right: 39px; bottom: 87px`).
const DOCK_RIGHT: f32 = 39.0;
const DOCK_BOTTOM: f32 = 87.0;

/// A flat-layer element's id is this plus a count of the elements that have entered the tree,
/// so that it never equals a card's, which is its object's slot in the view, or an ability's
/// on the stack (`table::ABILITY_IDS`).
const FIRST_ID: u32 = 1 << 16;

/// What taffy's measure function needs of a node that shows text.
#[derive(Clone, Debug)]
struct TextLeaf {
    text: String,
    style: TypeStyle,
}

/// One kept element, with its layout.
#[derive(Debug)]
struct Node {
    id: ElementId,
    /// The element as the screen function returned it, without its children.
    element: Element,
    taffy: NodeId,
    children: Vec<Node>,
    /// Its border box on screen, in logical points.
    rect: Rect,
    /// The box its text is laid out in: the border box less its padding.
    content: Rect,
    /// Its text, broken at the content box's width.
    block: Option<Block>,
}

/// The flat layer.
pub struct Layer {
    taffy: TaffyTree<TextLeaf>,
    /// The window, padded by the safe area: the root every screen is laid out in.
    root: NodeId,
    screen: Option<Node>,
    /// The floats, in order of their ids, which is the order they are drawn in.
    floats: Vec<Float>,
    /// Elements laid out ahead for floats, which a frame shows without laying anything out.
    prepared: Vec<Prepared>,
    next_id: u32,
    text: Text,
    window: Window,
    glyphs: Glyphs,
    /// Layout nodes computed and texts shaped since `take_work` was last called.
    layout_nodes: u32,
    text_runs: u32,
}

impl Default for Layer {
    fn default() -> Self {
        Self::new()
    }
}

impl Layer {
    #[must_use]
    pub fn new() -> Self {
        let mut taffy = TaffyTree::new();
        // Logical points need not be whole: a box rounded smaller than its text's measured
        // width would break the text onto another line.
        taffy.disable_rounding();
        let window = Window::default();
        let root = taffy
            .new_leaf(root_style(window))
            .expect("taffy makes a node");
        Self {
            taffy,
            root,
            screen: None,
            floats: Vec::new(),
            prepared: Vec::new(),
            next_id: FIRST_ID,
            text: Text::new(),
            window,
            glyphs: Glyphs::default(),
            layout_nodes: 0,
            text_runs: 0,
        }
    }

    /// Lays the screen out in a window of a new size, scale or safe area.
    pub fn resize(&mut self, window: Window) {
        if window == self.window {
            return;
        }
        let rescaled = window.scale.to_bits() != self.window.scale.to_bits();
        self.window = window;
        self.taffy
            .set_style(self.root, root_style(window))
            .expect("the root is in the tree");
        self.relayout();
        if rescaled {
            self.want_glyphs();
        }
    }

    /// Shows `screen`, or nothing, matching it against the screen shown before.
    pub fn show(&mut self, screen: Option<Element>) {
        let old = self.screen.take();
        self.screen = match (old, screen) {
            (old, Some(element)) => Some(self.reconcile(old, element)),
            (Some(old), None) => {
                self.remove(old);
                None
            }
            (None, None) => None,
        };
        let children: Vec<NodeId> = self.screen.iter().map(|node| node.taffy).collect();
        if self
            .taffy
            .children(self.root)
            .expect("the root is in the tree")
            != children
        {
            self.taffy
                .set_children(self.root, &children)
                .expect("the screen's node is in the tree");
        }
        self.relayout();
    }

    /// The layout nodes computed and texts shaped since the last call.
    pub fn take_work(&mut self) -> (u32, u32) {
        (
            std::mem::take(&mut self.layout_nodes),
            std::mem::take(&mut self.text_runs),
        )
    }

    /// The glyph table, which `Input::Assets` fills and the host drains of wanted glyphs.
    pub fn glyphs_mut(&mut self) -> &mut Glyphs {
        &mut self.glyphs
    }

    /// The innermost element at `point` that a press means something on, with its press and
    /// whether it is enabled. Floats are drawn over the screen, so they are searched first, the
    /// last drawn first. The screen is searched only while it is `open`, that is drawn.
    #[must_use]
    pub fn press_at(&self, point: glam::Vec2, open: bool) -> Option<(ElementId, Press, bool)> {
        fn find(node: &Node, point: glam::Vec2) -> Option<(ElementId, Press, bool)> {
            if !contains(node.rect, point) {
                return None;
            }
            (node.children.iter().rev())
                .find_map(|child| find(child, point))
                .or_else(|| {
                    let element = &node.element;
                    (element.press.clone()).map(|press| (node.id, press, element.enabled))
                })
        }
        (self.floats.iter().rev())
            .find_map(|float| find(&float.node, point - float.corner()))
            .or_else(|| {
                (self.screen.as_ref())
                    .filter(|_| open)
                    .and_then(|node| find(node, point))
            })
    }

    /// Matches `element` against `old`, the node at its place in the tree before: a node of the
    /// same role and name is kept, and updated where it differs.
    fn reconcile(&mut self, old: Option<Node>, mut element: Element) -> Node {
        let children = std::mem::take(&mut element.children);
        let mut node = match old {
            Some(old) if old.element.role == element.role && old.element.name == element.name => {
                old
            }
            old => {
                if let Some(old) = old {
                    self.remove(old);
                }
                let taffy = self
                    .taffy
                    .new_leaf(taffy_style(&element.style))
                    .expect("taffy makes a node");
                self.next_id += 1;
                Node {
                    id: ElementId(self.next_id - 1),
                    element: Element {
                        text: None,
                        ..element.clone()
                    },
                    taffy,
                    children: Vec::new(),
                    rect: Rect::default(),
                    content: Rect::default(),
                    block: None,
                }
            }
        };
        if node.element.style != element.style {
            self.taffy
                .set_style(node.taffy, taffy_style(&element.style))
                .expect("the node is in the tree");
        }
        let text_changed =
            node.element.text != element.text || node.element.style.text != element.style.text;
        if text_changed {
            let leaf = (element.text.clone()).map(|text| TextLeaf {
                text,
                style: element.style.text,
            });
            self.taffy
                .set_node_context(node.taffy, leaf)
                .expect("the node is in the tree");
            node.block = None;
        }
        if !node.element.same_node(&element) {
            node.element = element;
        }
        let mut old_children = std::mem::take(&mut node.children).into_iter();
        node.children = children
            .into_iter()
            .map(|child| {
                let old = old_children.next();
                self.reconcile(old, child)
            })
            .collect();
        for old in old_children {
            self.remove(old);
        }
        let ids: Vec<NodeId> = node.children.iter().map(|child| child.taffy).collect();
        if self
            .taffy
            .children(node.taffy)
            .expect("the node is in the tree")
            != ids
        {
            self.taffy
                .set_children(node.taffy, &ids)
                .expect("the children are in the tree");
        }
        node
    }

    /// Takes `node` and everything under it out of taffy's tree.
    fn remove(&mut self, node: Node) {
        for child in node.children {
            self.remove(child);
        }
        self.taffy
            .remove(node.taffy)
            .expect("the node is in the tree");
    }

    /// Computes the layout of whatever changed, and lays out text at its new widths. Does
    /// nothing when nothing has changed since the last call.
    fn relayout(&mut self) {
        let root_dirty = self
            .taffy
            .dirty(self.root)
            .expect("the root is in the tree");
        let floats_dirty: u32 = (self.floats.iter())
            .map(|float| self.dirty(&float.node))
            .sum();
        if !root_dirty && floats_dirty == 0 {
            return;
        }
        if root_dirty {
            self.layout_nodes += 1 + self.screen.as_ref().map_or(0, |node| self.dirty(node));
            let size = self.window.size;
            let space = taffy::Size {
                width: AvailableSpace::Definite(size.width),
                height: AvailableSpace::Definite(size.height),
            };
            compute(&mut self.taffy, &mut self.text, self.root, space);
            if let Some(mut node) = self.screen.take() {
                self.place(&mut node, glam::Vec2::ZERO);
                self.screen = Some(node);
            }
        }
        if floats_dirty > 0 {
            self.layout_nodes += floats_dirty;
            // A float is as large as its content, whatever the window's size.
            let space = taffy::Size {
                width: AvailableSpace::MaxContent,
                height: AvailableSpace::MaxContent,
            };
            let mut floats = std::mem::take(&mut self.floats);
            for float in &mut floats {
                let root = float.node.taffy;
                if self.taffy.dirty(root).expect("the float is in the tree") {
                    compute(&mut self.taffy, &mut self.text, root, space);
                    self.place(&mut float.node, glam::Vec2::ZERO);
                }
            }
            self.floats = floats;
        }
        // Only a pass over the screen drops the texts it did not use. A pass over floats alone
        // uses only their texts, and would drop the screen's, which taffy measures again when
        // any part of the screen changes, so they would all be shaped again.
        self.text_runs += if root_dirty {
            self.text.end_pass()
        } else {
            self.text.take_runs()
        };
        self.want_glyphs();
    }

    /// How many nodes at and under `node` taffy will recompute.
    fn dirty(&self, node: &Node) -> u32 {
        let own = u32::from(
            self.taffy
                .dirty(node.taffy)
                .expect("the node is in the tree"),
        );
        own + node
            .children
            .iter()
            .map(|child| self.dirty(child))
            .sum::<u32>()
    }

    /// Reads `node`'s computed box, with its parent's top left corner at `origin`, and breaks
    /// its text at its content box's width where that width has changed.
    fn place(&mut self, node: &mut Node, origin: glam::Vec2) {
        let layout = self
            .taffy
            .layout(node.taffy)
            .expect("the node is in the tree");
        let corner = origin + glam::Vec2::new(layout.location.x, layout.location.y);
        node.rect = Rect {
            x: corner.x,
            y: corner.y,
            width: layout.size.width,
            height: layout.size.height,
        };
        let (padding, border) = (layout.padding, layout.border);
        node.content = Rect {
            x: corner.x + padding.left + border.left,
            y: corner.y + padding.top + border.top,
            width: layout.content_box_width(),
            height: layout.content_box_height(),
        };
        if let Some(words) = &node.element.text {
            // A little room past the measured width, so that breaking at exactly that width
            // never wraps the last word for a rounding error.
            let limit = Some(node.content.width + 0.01);
            if node
                .block
                .as_ref()
                .is_none_or(|block| block.max_width != limit)
            {
                node.block = Some(
                    self.text
                        .block(words, node.element.style.text, limit)
                        .clone(),
                );
            }
        }
        for child in &mut node.children {
            self.place(child, corner);
        }
    }

    /// Asks for every glyph the laid-out text needs that no atlas page holds.
    fn want_glyphs(&mut self) {
        fn visit(node: &Node, scale: f32, glyphs: &mut Glyphs) {
            if let Some(block) = &node.block {
                glyphs.want(block, node.element.style.text, scale);
            }
            for child in &node.children {
                visit(child, scale, glyphs);
            }
        }
        let floats = self.floats.iter().map(|float| &float.node);
        for node in self.screen.iter().chain(floats) {
            visit(node, self.window.scale, &mut self.glyphs);
        }
    }
}

/// Computes the layout of the tree under `root` in `space`, measuring text with `text`.
fn compute(
    taffy: &mut TaffyTree<TextLeaf>,
    text: &mut Text,
    root: NodeId,
    space: taffy::Size<AvailableSpace>,
) {
    taffy
        .compute_layout_with_measure(root, space, |inputs, _, leaf, style| {
            compute_leaf_layout(
                inputs,
                style,
                |_, _| 0.0,
                |known, available| {
                    let Some(leaf) = leaf else {
                        return taffy::Size::ZERO;
                    };
                    let limit = known.width.or(match available.width {
                        AvailableSpace::Definite(width) => Some(width),
                        AvailableSpace::MinContent => Some(0.0),
                        AvailableSpace::MaxContent => None,
                    });
                    let (width, height) = text.measure(&leaf.text, leaf.style, limit);
                    taffy::Size {
                        width: known.width.unwrap_or(width),
                        height: known.height.unwrap_or(height),
                    }
                },
            )
        })
        .expect("the root is in the tree");
}

/// The root's style: the whole window, padded by its safe area and by the dock's offsets, with
/// the screen in its bottom right corner.
fn root_style(window: Window) -> taffy::Style {
    let inset = window.safe_area;
    taffy::Style {
        display: Display::Flex,
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::FLEX_END,
        justify_content: AlignContent::FLEX_END,
        size: size(Size {
            width: window.size.width,
            height: window.size.height,
        }),
        padding: taffy::Rect {
            top: LengthPercentage::length(inset.top),
            right: LengthPercentage::length(inset.right + DOCK_RIGHT),
            bottom: LengthPercentage::length(inset.bottom + DOCK_BOTTOM),
            left: LengthPercentage::length(inset.left),
        },
        ..taffy::Style::default()
    }
}

fn size(size: Size) -> taffy::Size<Dimension> {
    taffy::Size {
        width: Dimension::length(size.width),
        height: Dimension::length(size.height),
    }
}

/// An element's style as taffy's.
fn taffy_style(style: &Style) -> taffy::Style {
    let [top, right, bottom, left] = style.padding.map(LengthPercentage::length);
    let dimension = |value: Option<f32>| value.map_or(Dimension::auto(), Dimension::length);
    taffy::Style {
        display: Display::Flex,
        flex_direction: match style.direction {
            Direction::Row => FlexDirection::Row,
            Direction::Column => FlexDirection::Column,
        },
        gap: taffy::Size {
            width: LengthPercentage::length(style.gap),
            height: LengthPercentage::length(style.gap),
        },
        padding: taffy::Rect {
            top,
            right,
            bottom,
            left,
        },
        flex_grow: style.grow,
        align_items: match style.align_items {
            None => AlignItems::NORMAL,
            Some(Align::Start) => AlignItems::FLEX_START,
            Some(Align::Center) => AlignItems::CENTER,
            Some(Align::End) => AlignItems::FLEX_END,
            Some(Align::Stretch) => AlignItems::STRETCH,
        },
        justify_content: match style.justify_content {
            None => AlignContent::NORMAL,
            Some(Justify::Start) => AlignContent::FLEX_START,
            Some(Justify::Center) => AlignContent::CENTER,
            Some(Justify::End) => AlignContent::FLEX_END,
            Some(Justify::SpaceBetween) => AlignContent::SPACE_BETWEEN,
        },
        size: taffy::Size {
            width: dimension(style.width),
            height: dimension(style.height),
        },
        ..taffy::Style::default()
    }
}

fn contains(rect: Rect, point: glam::Vec2) -> bool {
    point.x >= rect.x
        && point.x <= rect.x + rect.width
        && point.y >= rect.y
        && point.y <= rect.y + rect.height
}

/// Whether an element of `role` is drawn on a panel: every button, and the top element of a
/// screen.
fn has_panel(role: Role, top: bool) -> bool {
    role == Role::Button || top
}

#[cfg(test)]
mod tests {
    use super::FIRST_ID;
    use crate::frame::LAYER_ELEMENTS;
    use crate::table::ABILITY_IDS;

    #[test]
    fn no_flat_layer_id_reaches_the_abilities_ids() {
        let last = u64::from(FIRST_ID) + LAYER_ELEMENTS as u64;
        assert!(last < u64::from(ABILITY_IDS));
    }
}
