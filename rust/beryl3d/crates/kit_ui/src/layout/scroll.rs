//! An element's scroll offset stays with its retained node. Scrolling moves the children
//! under the element's clip when the layer is written, without running layout again.

use glam::Vec2;

use super::{Layer, Node};
use crate::element::{Display, Overflow};
use crate::input::Key;
use crate::scroll::Viewport;
use kit_frame::frame::ElementId;

impl Layer {
    /// The innermost scrollable element at `point`, inside its ancestor clips.
    #[must_use]
    pub fn scroller_at(&self, point: Vec2, open: bool) -> Option<ElementId> {
        self.scroll_target_at(point, open, None).map(|(id, _)| id)
    }

    /// The innermost scroll viewport at `point`. For a wheel event, skip viewports that
    /// have no overflow along its axes. This uses the same clips as a press.
    #[must_use]
    pub fn scroll_target_at(
        &self,
        point: Vec2,
        open: bool,
        wheel: Option<Vec2>,
    ) -> Option<(ElementId, Viewport)> {
        self.hit(point, open, |node, _| {
            let viewport = node.viewport()?;
            wheel
                .is_none_or(|delta| viewport.takes_wheel(delta))
                .then_some((node.id, viewport))
        })
    }

    /// The innermost scroll viewport containing the focused element that takes `key`.
    #[must_use]
    pub fn scroll_key(&self, key: Key, open: bool) -> Option<(ElementId, Vec2)> {
        fn find(
            node: &Node,
            focused: ElementId,
            key: Key,
            open: bool,
            parent: Option<(ElementId, Vec2)>,
        ) -> Option<(ElementId, Vec2)> {
            if node.element.style.display == Display::None || (node.element.waits && !open) {
                return None;
            }
            let here = node
                .viewport()
                .and_then(|view| view.key(key))
                .map(|offset| (node.id, offset));
            let nearest = here.or(parent.filter(|_| node.element.anchor.is_none()));
            if node.id == focused {
                return nearest;
            }
            node.children
                .iter()
                .find_map(|child| find(child, focused, key, open, nearest))
        }
        let focused = self.focused?;
        self.screen
            .iter()
            .chain(self.floats.iter().map(|float| &float.node))
            .find_map(|root| find(root, focused, key, open, None))
    }

    /// The viewport of the element `id`, if it has scrollable overflow.
    #[must_use]
    pub fn scroll_viewport(&self, id: ElementId) -> Option<Viewport> {
        self.screen
            .iter()
            .chain(self.floats.iter().map(|float| &float.node))
            .find_map(|root| root.find(id))?
            .viewport()
    }

    /// The element's current offset, including zero while its content fits.
    #[must_use]
    pub fn scroll_offset(&self, id: ElementId) -> Option<Vec2> {
        self.screen
            .iter()
            .chain(self.floats.iter().map(|float| &float.node))
            .find_map(|root| root.find(id))
            .map(|node| node.scroll)
    }

    /// Sets the offset, clamped to the element's content. Returns whether it changed.
    pub fn set_scroll_offset(&mut self, id: ElementId, offset: Vec2) -> bool {
        let mut roots = self
            .screen
            .iter_mut()
            .chain(self.floats.iter_mut().map(|float| &mut float.node));
        roots
            .find_map(|node| node.find_mut(id))
            .is_some_and(|node| node.set_scroll_offset(offset))
    }

    /// Scrolls an element by `delta` points. Selection in a list uses this to show a row.
    pub fn scroll(&mut self, id: ElementId, delta: Vec2) -> bool {
        let Some(from) = self.scroll_offset(id) else {
            return false;
        };
        self.set_scroll_offset(id, from + delta)
    }

    /// Scrolls the screen's element named `name` to the end of its content.
    pub fn scroll_to_end(&mut self, name: &str) {
        fn find<'a>(node: &'a mut Node, name: &str) -> Option<&'a mut Node> {
            if node.is_scroller() && node.element.name == name {
                return Some(node);
            }
            node.children.iter_mut().find_map(|child| find(child, name))
        }
        if let Some(node) = self.screen.as_mut().and_then(|node| find(node, name)) {
            node.set_scroll_offset(node.most_scroll());
        }
    }
}

impl Node {
    fn is_scroller(&self) -> bool {
        matches!(
            self.element.style.overflow,
            Overflow::Scroll | Overflow::ScrollX | Overflow::ScrollY
        )
    }

    fn viewport(&self) -> Option<Viewport> {
        let limit = self.most_scroll();
        (limit != Vec2::ZERO && self.element.style.display != Display::None).then(|| {
            Viewport::points(
                self.scroll,
                limit,
                Vec2::new(self.rect.width, self.rect.height),
            )
        })
    }

    fn set_scroll_offset(&mut self, offset: Vec2) -> bool {
        let offset = offset.clamp(Vec2::ZERO, self.most_scroll());
        let changed = self.scroll != offset;
        self.scroll = offset;
        changed
    }

    /// How far the content reaches beyond the box on each scrollable axis.
    pub(super) fn most_scroll(&self) -> Vec2 {
        match self.element.style.overflow {
            Overflow::Scroll => self.scroll_room,
            Overflow::ScrollX => self.scroll_room * Vec2::X,
            Overflow::ScrollY => self.scroll_room * Vec2::Y,
            Overflow::Clip | Overflow::Visible => Vec2::ZERO,
        }
    }
}
