//! Scrolling (docs/beryl-3d.md, "Elements"): an element with `overflow: Scroll` keeps a scroll
//! offset, which the wheel and a touch drag move, clamped so that its content never leaves a
//! gap at either end. Scrolling changes no layout: the offset moves the element's children
//! when the layer is written, under the clip of the element's box (`write`).

use glam::Vec2;

use super::{Layer, Node};
use crate::element::Overflow;
use crate::frame::ElementId;

impl Layer {
    /// The innermost element at `point` that scrolls and has content past its box, if the
    /// screen is `open`.
    #[must_use]
    pub fn scroller_at(&self, point: Vec2, open: bool) -> Option<ElementId> {
        self.hit(point, open, |node, _| {
            (node.most_scroll() != Vec2::ZERO).then_some(node.id)
        })
    }

    /// Scrolls the element `id` by `delta` points, clamped to its content. Returns whether its
    /// offset changed.
    pub fn scroll(&mut self, id: ElementId, delta: Vec2) -> bool {
        let Some(node) = self.screen.as_mut().and_then(|node| node.find_mut(id)) else {
            return false;
        };
        let scroll = (node.scroll + delta).clamp(Vec2::ZERO, node.most_scroll());
        let changed = scroll != node.scroll;
        node.scroll = scroll;
        changed
    }
}

impl Layer {
    /// Scrolls the screen's element named `name` that scrolls to the end of its content, as
    /// far down and right as it goes. Does nothing if the screen has no such element.
    pub fn scroll_to_end(&mut self, name: &str) {
        fn find<'a>(node: &'a mut Node, name: &str) -> Option<&'a mut Node> {
            if node.element.style.overflow == Overflow::Scroll && node.element.name == name {
                return Some(node);
            }
            node.children.iter_mut().find_map(|child| find(child, name))
        }
        if let Some(node) = self.screen.as_mut().and_then(|node| find(node, name)) {
            node.scroll = node.most_scroll();
        }
    }
}

impl Node {
    /// How far this node can scroll on each axis: as far as its content reaches past its box,
    /// and nowhere unless it scrolls.
    pub(super) fn most_scroll(&self) -> Vec2 {
        if self.element.style.overflow != Overflow::Scroll {
            return Vec2::ZERO;
        }
        self.scroll_room
    }
}
