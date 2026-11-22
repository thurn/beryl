//! The flat layer's retained tree (docs/beryl-3d.md, "Elements", "Sizes" and "Text"). The
//! client hands it the elements a screen function returns, and it matches them against the
//! tree it keeps by role, name and place (`reconcile`): a node equal to the one before it keeps
//! its layout and its laid-out text. Layout is flexbox and grid, computed by taffy (`style`),
//! and text is shaped and broken by parley through taffy's measure function. All of that
//! happens in `Client::handle`; writing a frame only reads the numbers kept here. An element
//! with an anchor stands at a point of the window or on a scene object (`anchor`). Beside the
//! screen it keeps floats, small elements placed at a point of the window (`float`).

mod anchor;
mod edit;
mod fit;
mod float;
mod focus;
mod geometry;
mod menu;
mod motion;
mod objects;
mod panel;
mod pointed;
pub use pointed::PointerTarget;
mod reconcile;
mod scroll;
mod shadows;
mod shake;
pub use shadows::shape_radius;
pub use shake::Shake;
mod style;
mod tip;
mod write;

pub use write::FaceLayers;

use taffy::{
    AlignContent, AlignItems, AvailableSpace, Dimension, Display, FlexDirection, LengthPercentage,
    NodeId, TaffyTree, compute_leaf_layout,
};

pub use anchor::Spot;
pub use edit::{Edit, Edited, FieldChange, TextEntry};
pub use float::FloatLook;
pub use menu::{DismissMenu, Navigation};
pub use motion::Ticked;
pub use tip::TIP_PREFIX;

use float::{Float, Prepared};

use crate::element::{self, Action, Element};
use crate::input::{Rect, Size};
use crate::widgets::icons::Icons;
use kit_base::geometry::Window;
use kit_frame::frame::{ElementId, Role};
use kit_text::text::{Block, Glyphs, Text, Truncate, TypeStyle};

/// Where the desktop layout anchors a screen: its bottom right corner this far in from the
/// safe area's, where the web interface's action dock stands (`.dock` in
/// `ui/src/screens/styles/controls.css`: `right: 39px; bottom: 87px`).
pub const DOCK_RIGHT: f32 = 39.0;
pub const DOCK_BOTTOM: f32 = 87.0;

/// A flat-layer element's id is this plus a count of the elements that have entered the tree,
/// so that it never equals a card's, which is its object's slot in the view, or an ability's
/// on the stack (`table::ABILITY_IDS`).
const FIRST_ID: u32 = 1 << 16;

/// What taffy's measure function needs of a node that shows text.
#[derive(Clone, Debug)]
struct TextLeaf {
    text: String,
    style: TypeStyle,
    spans: Vec<element::Span>,
    /// Whether the text stays on one line (`Style::fit`), so that it is measured unbroken.
    fit: bool,
    /// How the text is cut to its box (`Style::truncate`).
    truncate: Option<Truncate>,
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
    /// For an element anchored to a scene object, the object's centre on the window as the
    /// last frame drew it (the middle of its box's top edge for `Anchor::Above`, and of its
    /// bottom edge for `Anchor::Below`), or `None` if the table did not show it
    /// (`Layer::follow`).
    followed: Option<glam::Vec2>,
    /// Where its layout put its top left corner, relative to its tree's, once it has been
    /// placed.
    location: Option<glam::Vec2>,
    /// How far what it holds reaches past its box on each axis, which is as far as it could
    /// scroll.
    scroll_room: glam::Vec2,
    /// How far its children are scrolled, for an element that scrolls (`scroll`).
    scroll: glam::Vec2,
    /// For text that shrinks to fit (`fit`), the content box width its block was fitted to.
    fitted: Option<f32>,
    /// Visual tracks sampled on the client's clock.
    tracks: motion::Tracks,
    /// Children leaving the tree. Their last layout is kept for painting.
    departing: Vec<Node>,
    /// For a menu, a submenu or a tooltip, where its top left corner stands on the window, once
    /// it has been placed (`menu`, `tip`).
    placed: Option<glam::Vec2>,
    /// Whether a submenu or a tooltip is shown now.
    shown: bool,
}

/// The flat layer.
pub struct Layer {
    tuning: kit_base::data::flat::Flat,
    taffy: TaffyTree<TextLeaf>,
    /// The window, padded by the safe area: the root every screen is laid out in.
    root: NodeId,
    screen: Option<Node>,
    screen_shows: usize,
    departing: Vec<Node>,
    /// The floats, in order of their ids, which is the order they are drawn in.
    floats: Vec<Float>,
    /// Elements laid out ahead for floats, which a frame shows without laying anything out.
    prepared: Vec<Prepared>,
    next_id: u32,
    text: Text,
    window: Window,
    glyphs: Glyphs,
    icons: Icons,
    /// Layout nodes computed and texts shaped since `take_work` was last called.
    layout_nodes: u32,
    text_runs: u32,
    /// The element that takes the keys (`focus`).
    focused: Option<ElementId>,
    /// The element a press under the pointer would reach, and the element held down by a
    /// press that has not ended, which the widgets are drawn hovered and pressed by
    /// (`Layer::point_at`).
    hovered: Option<ElementId>,
    pressed: Option<ElementId>,
    /// The focused text field's editor, while a text field has the focus (`edit`).
    editing: Option<edit::Editing>,
    text_fields: edit::TextFields,
    /// Whether the tree being matched and laid out is the screen's, whose changes transition
    /// (`motion`); floats' do not.
    animating: bool,
    /// The times of the first and the latest frames that ticked the layer (`Layer::tick`), in
    /// nanoseconds since the client started. A change found while the clock still reads the
    /// first frame's time does not transition (`Layer::started`).
    ticks: Option<(u64, u64)>,
    /// The menu row whose submenu is open (`menu`).
    submenu: Option<ElementId>,
    /// The menu row the pointer is on, which it highlighted when it came onto it (`tip`).
    pointed: Option<ElementId>,
    /// The press whose elements are shaken, and how far across the screen they are moved now, in
    /// logical points (`Layer::shake`).
    shaken: Option<(Action, f32)>,
    /// The tooltip the pointer is resting toward, if any (`tip`).
    tip: Option<tip::Resting>,
}

impl Default for Layer {
    fn default() -> Self {
        Self::new()
    }
}

impl Layer {
    #[must_use]
    pub fn new() -> Self {
        Self::with_tuning(kit_base::data::flat::Flat::shipped())
    }

    #[must_use]
    pub fn with_tuning(tuning: kit_base::data::flat::Flat) -> Self {
        let mut taffy = TaffyTree::new();
        // Logical points need not be whole: a box rounded smaller than its text's measured
        // width would break the text onto another line.
        taffy.disable_rounding();
        let window = Window::default();
        let root = taffy
            .new_leaf(root_style(window))
            .expect("taffy makes a node");
        Self {
            tuning,
            taffy,
            root,
            screen: None,
            screen_shows: 0,
            departing: Vec::new(),
            floats: Vec::new(),
            prepared: Vec::new(),
            next_id: FIRST_ID,
            text: Text::new(),
            window,
            glyphs: Glyphs::default(),
            icons: Icons::default(),
            layout_nodes: 0,
            text_runs: 0,
            focused: None,
            hovered: None,
            pressed: None,
            editing: None,
            text_fields: edit::TextFields::default(),
            animating: false,
            ticks: None,
            submenu: None,
            pointed: None,
            shaken: None,
            tip: None,
        }
    }

    /// Updates the shared flat-layer metrics and repositions any open menus.
    pub fn set_tuning(&mut self, tuning: kit_base::data::flat::Flat) {
        let edge_changed = self.tuning.menu_edge.to_bits() != tuning.menu_edge.to_bits();
        self.tuning = tuning;
        if edge_changed && let Some(node) = &mut self.screen {
            let tree = node.corner().unwrap_or(glam::Vec2::ZERO);
            menu::place_menus(node, tree, None, self.window.size, tuning.menu_edge);
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

    /// The screen's kept node ids, in the element tree's depth-first order.
    pub fn element_ids(&self) -> Vec<ElementId> {
        fn visit(node: &Node, ids: &mut Vec<ElementId>) {
            ids.push(node.id);
            for child in &node.children {
                visit(child, ids);
            }
        }
        let mut ids = Vec::new();
        if let Some(screen) = &self.screen {
            visit(screen, &mut ids);
        }
        ids
    }

    /// Whether a screen is shown: false until the first `show` of one, and after `show(None)`.
    #[must_use]
    pub fn showing(&self) -> bool {
        self.screen.is_some()
    }

    /// Shows `screen`, or nothing, matching it against the screen shown before.
    ///
    /// # Panics
    ///
    /// When two elements present the same table object or a host depends on table placement.
    pub fn show(&mut self, screen: Option<Element>) {
        self.screen_shows = screen.as_ref().map_or(0, objects::validate_screen_shows);
        self.animating = self.started();
        let old = self.screen.take();
        self.screen = match (old, screen) {
            (old, Some(element)) => Some(self.reconcile(old, element)),
            (Some(old), None) => {
                if let Some(old) = self.depart(old) {
                    self.departing.push(old);
                }
                None
            }
            (None, None) => None,
        };
        // An anchored screen is a taffy tree of its own, outside the root's flow.
        let children: Vec<NodeId> = (self.screen.iter())
            .filter(|node| node.element.anchor.is_none())
            .map(|node| node.taffy)
            .collect();
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
        self.animating = false;
        self.menus_shown();
        self.follow_focus();
    }

    /// The layout nodes computed and texts shaped since the last call.
    pub fn take_work(&mut self) -> (u32, u32) {
        (
            std::mem::take(&mut self.layout_nodes),
            std::mem::take(&mut self.text_runs),
        )
    }

    /// The glyph table.
    #[must_use]
    pub fn glyphs(&self) -> &Glyphs {
        &self.glyphs
    }

    /// The glyph table, which `Input::Assets` fills and the host drains of wanted glyphs.
    pub fn glyphs_mut(&mut self) -> &mut Glyphs {
        &mut self.glyphs
    }

    /// The icon table, which `Input::Assets` fills.
    #[must_use]
    pub fn icons(&self) -> &Icons {
        &self.icons
    }

    pub fn icons_mut(&mut self) -> &mut Icons {
        &mut self.icons
    }

    /// The fonts and shaped texts, with the glyph table, for text the table draws outside the
    /// flat layer.
    pub fn text_and_glyphs(&mut self) -> (&mut Text, &mut Glyphs) {
        (&mut self.text, &mut self.glyphs)
    }

    /// The innermost element at `point` that a press means something on, with its press and
    /// whether it is enabled. Elements are searched the last drawn first: the floats drawn over
    /// the screen, then the screen, then the floats drawn under it (`FloatId::under_screen`).
    /// What waits for the table to settle (`Element::waits`) is searched only while the layer
    /// is `open`, that is while it is drawn.
    #[must_use]
    pub fn press_at(&self, point: glam::Vec2, open: bool) -> Option<(ElementId, Action, bool)> {
        self.hit(point, open, |node, _| {
            let element = &node.element;
            (element.press.clone()).map(|press| (node.id, press, element.enabled))
        })
    }

    /// Takes `node` and everything under it out of taffy's tree.
    fn remove_layout(&mut self, node: &Node) {
        for child in &node.children {
            self.remove_layout(child);
        }
        self.taffy
            .remove(node.taffy)
            .expect("the node is in the tree");
    }

    /// Detaches a node from layout immediately, retaining its last boxes if it has a leave.
    fn depart(&mut self, mut node: Node) -> Option<Node> {
        self.remove_layout(&node);
        let presence = node.element.style.leave.filter(|_| self.animating)?;
        node.presence(presence, false);
        node.leaving().then_some(node)
    }

    /// Computes the layout of whatever changed, and lays out text at its new widths. Does
    /// nothing when nothing has changed since the last call.
    fn relayout(&mut self) {
        let root_dirty = self
            .taffy
            .dirty(self.root)
            .expect("the root is in the tree");
        let anchored_dirty = (self.screen.as_ref()).is_some_and(|node| self.anchored_dirty(node));
        let floats_dirty: u32 = (self.floats.iter())
            .map(|float| self.dirty(&float.node))
            .sum();
        if !root_dirty && !anchored_dirty && floats_dirty == 0 {
            return;
        }
        if root_dirty || anchored_dirty {
            self.layout_nodes +=
                u32::from(root_dirty) + self.screen.as_ref().map_or(0, |node| self.dirty(node));
            if root_dirty {
                let size = self.window.size;
                let space = taffy::Size {
                    width: AvailableSpace::Definite(size.width),
                    height: AvailableSpace::Definite(size.height),
                };
                compute(&mut self.taffy, &mut self.text, self.root, space);
            }
            if let Some(mut node) = self.screen.take() {
                if node.element.anchor.is_some()
                    && self
                        .taffy
                        .dirty(node.taffy)
                        .expect("the node is in the tree")
                {
                    compute(&mut self.taffy, &mut self.text, node.taffy, CONTENT);
                }
                compute_anchored(&mut self.taffy, &mut self.text, &node);
                self.place(&mut node, glam::Vec2::ZERO, false, false);
                let tree = node.corner().unwrap_or(glam::Vec2::ZERO);
                menu::place_menus(
                    &mut node,
                    tree,
                    None,
                    self.window.size,
                    self.tuning.menu_edge,
                );
                self.screen = Some(node);
            }
        }
        if floats_dirty > 0 {
            self.layout_nodes += floats_dirty;
            let mut floats = std::mem::take(&mut self.floats);
            for float in &mut floats {
                if self.dirty(&float.node) == 0 {
                    continue;
                }
                let root = float.node.taffy;
                if self.taffy.dirty(root).expect("the float is in the tree") {
                    compute(&mut self.taffy, &mut self.text, root, CONTENT);
                }
                compute_anchored(&mut self.taffy, &mut self.text, &float.node);
                self.place(&mut float.node, glam::Vec2::ZERO, false, false);
            }
            self.floats = floats;
        }
        // Only a pass over the screen drops the texts it did not use. A pass over floats alone
        // uses only their texts, and would drop the screen's, which taffy measures again when
        // any part of the screen changes, so they would all be shaped again.
        // Layout may have broken the edited field's own text at a new width in place of its
        // editor's.
        self.refresh_edit();
        self.text_runs += if root_dirty {
            self.text.end_pass()
        } else {
            self.text.take_runs()
        };
        self.want_glyphs();
    }

    /// Whether `node` or any node under it is anchored, and so a taffy tree of its own, and
    /// has changed since it was laid out.
    fn anchored_dirty(&self, node: &Node) -> bool {
        (node.element.anchor.is_some()
            && self
                .taffy
                .dirty(node.taffy)
                .expect("the node is in the tree"))
            || node.children.iter().any(|child| self.anchored_dirty(child))
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
    /// its text at its content box's width where that width has changed. `unseen` says that no
    /// frame has drawn the node's parent yet, because it or an ancestor is fading in and no
    /// frame has begun that fade. `at_once` says that the node and everything under it move to
    /// their new places without sliding, because a sibling in the same flow changed size.
    fn place(&mut self, node: &mut Node, origin: glam::Vec2, unseen: bool, at_once: bool) {
        let layout = self
            .taffy
            .layout(node.taffy)
            .expect("the node is in the tree");
        let corner = origin + glam::Vec2::new(layout.location.x, layout.location.y);
        node.scroll_room = glam::Vec2::new(layout.scroll_width(), layout.scroll_height());
        // Content that shrank, or a box that grew, leaves less to scroll: an offset past the new
        // end would show a gap after the last row.
        node.scroll = node.scroll.clamp(glam::Vec2::ZERO, node.most_scroll());
        // A kept node of the screen that its layout moves on the window moves there. A node
        // that stays where it was stays still, whatever its parent does, and one whose size
        // changes is drawn at its new place at once, since its size does not transition. A node
        // that no frame has drawn yet has no place on the window to move from: a screen built
        // and then built again before the next frame, as the start screen is when the host's
        // agents arrive, is drawn at its last layout, fading in.
        let unseen = unseen || node.unseen();
        let resized = node.rect.width.to_bits() != layout.size.width.to_bits()
            || node.rect.height.to_bits() != layout.size.height.to_bits();
        if self.animating
            && let Some(timing) = node.element.style.layout_slide
            && let Some(was) = node.location
            && was != corner
            && !resized
            && !unseen
            && !at_once
        {
            node.slide_layout(was - corner, timing);
        }
        node.location = Some(corner);
        node.rect = Rect {
            x: corner.x,
            y: corner.y,
            width: layout.size.width,
            height: layout.size.height,
        };
        node.finish_size_tracks(glam::Vec2::new(layout.size.width, layout.size.height));
        let (padding, border) = (layout.padding, layout.border);
        node.content = Rect {
            x: corner.x + padding.left + border.left,
            y: corner.y + padding.top + border.top,
            width: layout.content_box_width(),
            height: layout.content_box_height(),
        };
        if node.element.style.fit {
            self.fit(node);
        } else if let Some(words) = &node.element.text {
            // A little room past the measured width, so that breaking at exactly that width
            // never wraps the last word for a rounding error.
            let limit = Some(node.content.width + 0.01);
            if node
                .block
                .as_ref()
                .is_none_or(|block| block.max_width != limit)
            {
                let (style, spans) = (node.element.style.text, &node.element.spans);
                node.block = Some(match node.element.style.truncate {
                    Some(truncate) => self.text.truncated(words, style, spans, limit, truncate),
                    None => self.text.block(words, style, spans, limit).clone(),
                });
            }
        }
        // A child that entered or changed size takes its new box at once, since sizes do not
        // transition, so siblings that slide toward their new places would start inside it: in
        // a row of mana counters, the blue counter growing from "{U}" to "{U}2" ran 5 points
        // into the black counter as that slid right. Every child of the flow moves at once
        // instead, as the web's flex rows do. An anchored child stands apart from the flow.
        let reflowed = node.children.iter().any(|child| {
            child.element.anchor.is_none() && {
                let size = self
                    .taffy
                    .layout(child.taffy)
                    .expect("the node is in the tree")
                    .size;
                child.location.is_none()
                    || child.rect.width.to_bits() != size.width.to_bits()
                    || child.rect.height.to_bits() != size.height.to_bits()
            }
        });
        for child in &mut node.children {
            // An anchored child's tree is laid out from (0, 0), and stands at its corner.
            let (origin, child_at_once) = if child.element.anchor.is_some() {
                (glam::Vec2::ZERO, at_once)
            } else {
                (corner, at_once || reflowed)
            };
            self.place(child, origin, unseen, child_at_once);
        }
    }

    /// Asks for every glyph the laid-out text needs that no atlas page holds: the screen's, the
    /// floats' and the elements prepared for floats.
    fn want_glyphs(&mut self) {
        let floats = self.floats.iter().map(|float| &float.node);
        let prepared = self.prepared.iter().map(|prepared| &prepared.node);
        for node in self.screen.iter().chain(floats).chain(prepared) {
            want_node(node, self.window.scale, &mut self.glyphs);
        }
    }
}

impl Node {
    /// The node `id` at or under this one.
    fn find(&self, id: ElementId) -> Option<&Node> {
        if self.id == id {
            return Some(self);
        }
        self.children.iter().find_map(|child| child.find(id))
    }

    /// The node `id` at or under this one, to change.
    fn find_mut(&mut self, id: ElementId) -> Option<&mut Node> {
        if self.id == id {
            return Some(self);
        }
        self.children
            .iter_mut()
            .find_map(|child| child.find_mut(id))
    }
}

/// Asks `glyphs` for the glyphs of the text at and under `node` at `scale`.
fn want_node(node: &Node, scale: f32, glyphs: &mut Glyphs) {
    if let Some(block) = &node.block {
        glyphs.want_with(block, scale, &node.element.style.drop_shadow);
    }
    for child in &node.children {
        want_node(child, scale, glyphs);
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
                    let limit = if leaf.fit { None } else { limit };
                    let (width, height) = match leaf.truncate.filter(|_| !leaf.fit) {
                        Some(truncate) => {
                            let block = text.truncated(
                                &leaf.text,
                                leaf.style,
                                &leaf.spans,
                                limit,
                                truncate,
                            );
                            (block.width, block.height)
                        }
                        None => text.measure(&leaf.text, leaf.style, &leaf.spans, limit),
                    };
                    taffy::Size {
                        width: known.width.unwrap_or(width),
                        height: known.height.unwrap_or(height),
                    }
                },
            )
        })
        .expect("the root is in the tree");
}

/// The room an anchored element or a float is laid out in: as much as its content takes,
/// whatever the window's size.
const CONTENT: taffy::Size<AvailableSpace> = taffy::Size {
    width: AvailableSpace::MaxContent,
    height: AvailableSpace::MaxContent,
};

/// Computes the layout of every anchored tree under `node` that has changed. Each is a taffy
/// tree of its own, laid out at its content's size.
fn compute_anchored(taffy: &mut TaffyTree<TextLeaf>, text: &mut Text, node: &Node) {
    for child in &node.children {
        if child.element.anchor.is_some() && taffy.dirty(child.taffy).expect("in the tree") {
            compute(taffy, text, child.taffy, CONTENT);
        }
        compute_anchored(taffy, text, child);
    }
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

/// How far down `node`'s content box its laid-out text of `height` starts: a text field's at
/// the top, as a browser's text area's does, and every other text centred.
fn text_top(node: &Node, height: f32) -> f32 {
    if node.element.role == Role::Field {
        0.0
    } else {
        (node.content.height - height) / 2.0
    }
}

/// Whether an element of `role` is drawn on a panel: every button, and the top element of a
/// screen.
fn has_panel(role: Role, top: bool) -> bool {
    role == Role::Button || top
}

#[cfg(test)]
mod tests {
    use super::FIRST_ID;
    use kit_frame::frame::{ABILITY_IDS, LAYER_ELEMENTS};

    #[test]
    fn no_flat_layer_id_reaches_the_abilities_ids() {
        let last = u64::from(FIRST_ID) + LAYER_ELEMENTS as u64;
        assert!(last < u64::from(ABILITY_IDS));
    }
}
