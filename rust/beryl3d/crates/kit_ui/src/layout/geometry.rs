//! Flat geometry in logical points, shared by painting and scene placement.

use glam::Vec2;
use kit_base::geometry::plane::Zoom;
use kit_frame::frame::{ScreenClip, StateBits};

use super::{Layer, Node, panel::mask_of, write::moved};
use crate::element::{Gradient, Overflow};
use crate::input::Rect;

#[derive(Clone, Copy)]
pub(super) struct Geometry {
    pub point: Vec2,
    pub sink: Vec2,
    pub rect: Rect,
    pub painted: Rect,
    pub zoom: Zoom,
    pub scaled: bool,
}

impl Layer {
    /// Finds the first screen element named `name` in tree order.
    #[must_use]
    pub fn screen_element(&self, name: &str) -> Option<kit_frame::frame::ElementId> {
        fn find(node: &Node, name: &str) -> Option<kit_frame::frame::ElementId> {
            if node.element.name == name {
                return Some(node.id);
            }
            node.children.iter().find_map(|child| find(child, name))
        }
        self.screen.as_ref().and_then(|node| find(node, name))
    }

    /// Sets the pose of a decorative element and its children without changing their layout.
    pub fn pose_element(
        &mut self,
        element: Option<kit_frame::frame::ElementId>,
        offset: Vec2,
        scale: Vec2,
    ) {
        self.element_pose = element.map(|id| (id, offset, scale));
    }

    pub(super) fn geometry(
        &self,
        node: &Node,
        point: Vec2,
        zoom: Zoom,
        state: StateBits,
    ) -> Geometry {
        let pose = self.element_pose.filter(|(id, _, _)| *id == node.id);
        let point = point + node.translation() + pose.map_or(Vec2::ZERO, |(_, offset, _)| offset);
        let sink = self.node_sink(node, state);
        let rect = moved(node.paint_rect(), point + node.offset() + sink);
        let scale = node.scale().max(0.0) * pose.map_or(Vec2::ONE, |(_, _, scale)| scale);
        let scaled = scale != Vec2::ONE;
        let zoom = if scaled {
            let pivot = Vec2::new(rect.x, rect.y)
                + Vec2::new(rect.width, rect.height) * node.element.style.scale_origin;
            zoom.inside(pivot, scale)
        } else {
            zoom
        };
        Geometry {
            point,
            sink,
            rect,
            painted: zoom.rect(rect),
            zoom,
            scaled,
        }
    }
}

impl Geometry {
    /// A node's own mask clips its panel, contents, and children.
    pub(super) fn mask(
        self,
        mask: Option<Gradient>,
        inherited: Option<ScreenClip>,
    ) -> Option<ScreenClip> {
        mask.and_then(|mask| ScreenClip::masked(self.painted, inherited, mask))
    }

    /// Overflow clips only the children. When the box already has a mask, the clip includes
    /// the same rectangle.
    pub(super) fn inside_clip(
        self,
        node: &Node,
        masked: Option<ScreenClip>,
        inherited: Option<ScreenClip>,
    ) -> Option<ScreenClip> {
        masked.or_else(|| {
            if node.element.style.overflow == Overflow::Visible {
                inherited
            } else {
                Some(ScreenClip::cut(self.painted, inherited))
            }
        })
    }

    pub(super) fn inside(self, node: &Node) -> Vec2 {
        if node.element.style.overflow == Overflow::Visible {
            self.point
        } else {
            self.point - node.scroll
        }
    }
}

impl Node {
    pub(super) fn mask(&self) -> Option<Gradient> {
        mask_of(self)
    }
}
