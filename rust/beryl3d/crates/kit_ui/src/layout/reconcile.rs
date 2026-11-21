//! Matching the element tree a screen function returns against the tree the layer keeps
//! (docs/beryl-3d.md, "Elements"). A kept node keeps its id, its taffy node and its laid-out
//! text, so an element that is the same as before costs no layout and no shaping, and a tree
//! printout or a check that follows an element by its id still finds it after its siblings
//! move.

use taffy::{NodeId, TraversePartialTree};

use super::style::{same_layout, taffy_style};
use super::{Layer, Node, TextLeaf};
use crate::element::{Anchor, Element};
use crate::input::Rect;
use kit_frame::frame::ElementId;

impl Layer {
    /// Matches `element` against `old`, the node matched to it before: a node of the same role
    /// and name is kept, and updated where it differs; anything else is replaced by a new node.
    pub(super) fn reconcile(&mut self, old: Option<Node>, mut element: Element) -> Node {
        let mut node = match old {
            Some(old) if same_identity(&old.element, &element) => old,
            old => {
                if let Some(old) = old.and_then(|old| self.depart(old)) {
                    self.departing.push(old);
                }
                let children = std::mem::take(&mut element.children);
                let mut node = self.new_node(&element);
                element.children = children;
                self.update(&mut node, element);
                if self.animating
                    && node.element.anchor != Some(Anchor::Tip)
                    && let Some(enter) = node.element.style.enter
                {
                    node.presence(enter, true);
                    if enter.grouped {
                        for child in &mut node.children {
                            if child.element.style.enter.is_some_and(|spec| spec.grouped) {
                                child.stop_entrance_fade();
                            }
                        }
                    }
                }
                return node;
            }
        };
        self.update(&mut node, element);
        node
    }

    /// Shows `element` in place of the screen's node with the same role and name, matching it
    /// against that node as `show` matches a whole screen, so that a part of the screen that
    /// changes on its own (the step row, as the table reaches a step of the turn) is built
    /// again without the rest. Returns false, and changes nothing, when the screen has no such
    /// node.
    ///
    /// Unlike `show`, it changes the node at once: nothing fades or slides. The web fades a step
    /// icon's opacity over 120 ms, but a fade that starts as a step begins keeps the layer
    /// animating after the table has come to rest, which made the interface tests draw a
    /// tenth more frames.
    pub fn replace(&mut self, element: Element) -> bool {
        let Some(mut screen) = self.screen.take() else {
            return false;
        };
        let mut element = Some(element);
        let found = self.replace_in(&mut screen, &mut element);
        self.screen = Some(screen);
        if found {
            self.relayout();
        }
        found
    }

    /// Finds, depth first among `node`'s descendants, the first node with `element`'s role and
    /// name, and updates it to show `element`. Returns whether it found one.
    fn replace_in(&mut self, node: &mut Node, element: &mut Option<Element>) -> bool {
        for child in &mut node.children {
            if element
                .as_ref()
                .is_some_and(|element| same_identity(&child.element, element))
            {
                if let Some(element) = element.take() {
                    self.update(child, element);
                }
                return true;
            }
            if self.replace_in(child, element) {
                return true;
            }
        }
        false
    }

    /// A node for `element` that holds none of its text or children yet, so that `update`
    /// gives it them.
    fn new_node(&mut self, element: &Element) -> Node {
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
            followed: None,
            location: None,
            scroll_room: glam::Vec2::ZERO,
            scroll: glam::Vec2::ZERO,
            fitted: None,
            tracks: super::motion::Tracks::default(),
            departing: Vec::new(),
            placed: None,
            shown: false,
        }
    }

    /// Makes `node`, whose role and name equal `element`'s, show `element`, changing in taffy
    /// only what differs.
    fn update(&mut self, node: &mut Node, mut element: Element) {
        let children = std::mem::take(&mut element.children);
        let style_changed = node.element.style != element.style;
        // The step row changes its look inside a frame, so comparing layout must avoid
        // allocating the grid's track lists. A text shadow change still needs new glyphs.
        if style_changed
            && (!same_layout(&node.element.style, &element.style)
                || (element.text.is_some()
                    && node.element.style.drop_shadow != element.style.drop_shadow))
        {
            self.taffy
                .set_style(node.taffy, taffy_style(&element.style))
                .expect("the node is in the tree");
        }
        let text_changed = node.element.text != element.text
            || node.element.style.text != element.style.text
            || node.element.spans != element.spans
            || node.element.style.fit != element.style.fit;
        if text_changed {
            let leaf = (element.text.clone()).map(|text| TextLeaf {
                text,
                style: element.style.text,
                spans: element.spans.clone(),
                fit: element.style.fit,
            });
            self.taffy
                .set_node_context(node.taffy, leaf)
                .expect("the node is in the tree");
            node.block = None;
        }
        if !matches!(
            element.anchor,
            Some(Anchor::Object(..) | Anchor::Above(..) | Anchor::Below(..))
        ) {
            node.followed = None;
        }
        if style_changed {
            node.update_tracks(&element.style, element.anchor.is_some(), self.animating);
        }
        // A node that gains or loses its anchor leaves or joins its parent's flow, so its
        // layout is computed again in its new tree.
        if node.element.anchor.is_some() != element.anchor.is_some() {
            self.taffy
                .mark_dirty(node.taffy)
                .expect("the node is in the tree");
        }
        if !node.element.same_node(&element) {
            node.element = element;
        }
        if node.element.anchor == Some(Anchor::Tip)
            && let Some(enter) = node.element.style.enter
        {
            node.tracks.reserve_presence(&enter);
        }
        self.reconcile_children(node, children);
        self.link_children(node);
    }

    /// Matches `children` against `node`'s children. A child is matched to the old child with
    /// the same role and name; a child with no name is matched to the old child with no name
    /// at the same place among those with no name. When every child is where it was, they are
    /// matched in one pass that allocates nothing.
    fn reconcile_children(&mut self, node: &mut Node, children: Vec<Element>) {
        let in_order = node.children.len() == children.len()
            && (node.children.iter())
                .zip(&children)
                .all(|(old, new)| same_identity(&old.element, new));
        if in_order {
            for (old, element) in node.children.iter_mut().zip(children) {
                self.update(old, element);
            }
            return;
        }
        let mut old: Vec<Option<Node>> = std::mem::take(&mut node.children)
            .into_iter()
            .map(Some)
            .collect();
        let unnamed: Vec<usize> = (old.iter().enumerate())
            .filter(|(_, child)| child.as_ref().is_some_and(|c| c.element.name.is_empty()))
            .map(|(index, _)| index)
            .collect();
        let mut unnamed = unnamed.into_iter();
        let mut matched = Vec::with_capacity(children.len());
        for element in children {
            let found = if element.name.is_empty() {
                unnamed.next()
            } else {
                old.iter().position(|child| {
                    child
                        .as_ref()
                        .is_some_and(|child| same_identity(&child.element, &element))
                })
            };
            let kept = found.and_then(|index| old[index].take());
            let kept = match kept {
                Some(old) if !same_identity(&old.element, &element) => {
                    if let Some(old) = self.depart(old) {
                        node.departing.push(old);
                    }
                    None
                }
                kept => kept,
            };
            matched.push(self.reconcile(kept, element));
        }
        for left in old.into_iter().flatten() {
            if let Some(left) = self.depart(left) {
                node.departing.push(left);
            }
        }
        node.children = matched;
    }

    /// Gives `node`'s taffy node its children in the flow, which are its children that are not
    /// anchored, if they differ from the ones it has. An anchored child is a taffy tree of its
    /// own (`Layer::relayout`). Allocates nothing when they are the same.
    fn link_children(&mut self, node: &Node) {
        let flow = || (node.children.iter()).filter(|child| child.element.anchor.is_none());
        let count = self.taffy.child_count(node.taffy);
        let same = flow().count() == count
            && flow().enumerate().all(|(index, child)| {
                self.taffy.child_at_index(node.taffy, index).ok() == Some(child.taffy)
            });
        if !same {
            let ids: Vec<NodeId> = flow().map(|child| child.taffy).collect();
            self.taffy
                .set_children(node.taffy, &ids)
                .expect("the children are in the tree");
        }
    }
}

/// Whether a kept node shown as `old` may show `new`: the two have the same role and name.
fn same_identity(old: &Element, new: &Element) -> bool {
    old.role == new.role && old.name == new.name
}
