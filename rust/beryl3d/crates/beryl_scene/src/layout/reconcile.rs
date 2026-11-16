//! Matching the element tree a screen function returns against the tree the layer keeps
//! (docs/beryl-3d.md, "Elements"). A kept node keeps its id, its taffy node and its laid-out
//! text, so an element that is the same as before costs no layout and no shaping, and a tree
//! printout or a check that follows an element by its id still finds it after its siblings
//! move.

use taffy::{NodeId, TraversePartialTree};

use super::motion::Motion;
use super::style::taffy_style;
use super::{Layer, Node, TextLeaf};
use crate::element::{Anchor, Element};
use crate::frame::ElementId;
use crate::input::Rect;

impl Layer {
    /// Matches `element` against `old`, the node matched to it before: a node of the same role
    /// and name is kept, and updated where it differs; anything else is replaced by a new node.
    pub(super) fn reconcile(&mut self, old: Option<Node>, mut element: Element) -> Node {
        let mut node = match old {
            Some(old) if same_identity(&old.element, &element) => old,
            old => {
                if let Some(old) = old {
                    self.remove(old);
                }
                let children = std::mem::take(&mut element.children);
                let mut node = self.new_node(&element);
                element.children = children;
                self.update(&mut node, element);
                // A new node of the screen fades in, and its new children with it rather than
                // each on its own.
                if self.animating {
                    node.motion = Some(Motion::fade_in());
                    for child in &mut node.children {
                        child.motion = None;
                    }
                }
                return node;
            }
        };
        self.update(&mut node, element);
        node
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
            motion: None,
        }
    }

    /// Makes `node`, whose role and name equal `element`'s, show `element`, changing in taffy
    /// only what differs.
    fn update(&mut self, node: &mut Node, mut element: Element) {
        let children = std::mem::take(&mut element.children);
        if node.element.style != element.style {
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
        if !matches!(element.anchor, Some(Anchor::Object(..))) {
            node.followed = None;
        }
        // A kept node of the screen whose opacity changes fades to it from how opaque it is.
        if self.animating && node.element.style.opacity.to_bits() != element.style.opacity.to_bits()
        {
            let opacity = node.opacity();
            node.transition(glam::Vec2::ZERO, opacity);
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
            matched.push(self.reconcile(kept, element));
        }
        for left in old.into_iter().flatten() {
            self.remove(left);
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
