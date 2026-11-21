//! The laid-out screen nodes that declare a Show relation.

use glam::Vec2;
use kit_frame::frame::{FrameBuilder, Show};

use super::write::{intersection, moved};
use super::{Layer, Node};
use crate::element::{Display, Overflow};
use crate::input::Rect;

impl Layer {
    /// Visits the active screen's Show declarations with their boxes and inherited clips in
    /// logical points. Objects outside their clip still belong to the declaring elements.
    pub fn visit_shows(&self, mut visit: impl FnMut(&Show, Rect, Option<Rect>)) {
        if let Some(screen) = &self.screen {
            let origin = match screen.element.anchor {
                Some(_) => screen.corner(),
                None => Some(Vec2::ZERO),
            };
            if let Some(origin) = origin {
                collect(screen, origin, None, &mut visit);
            }
        }
    }
}

impl Node {
    pub(super) fn record_show(&self, index: u32, builder: &mut FrameBuilder) {
        if let Some(object) = self.element.show.as_ref().and_then(Show::object_id) {
            builder.shows(index, object);
        }
    }
}

fn collect(
    node: &Node,
    origin: Vec2,
    clip: Option<Rect>,
    visit: &mut impl FnMut(&Show, Rect, Option<Rect>),
) {
    if node.element.style.display == Display::None {
        return;
    }
    let origin = origin + node.translation();
    let rect = moved(node.paint_rect(), origin + node.offset());
    if let Some(show) = &node.element.show {
        visit(show, rect, clip);
    }
    let clip = if node.element.style.overflow == Overflow::Visible {
        clip
    } else {
        Some(clip.map_or(rect, |clip| {
            intersection(clip, rect).unwrap_or(Rect {
                width: 0.0,
                height: 0.0,
                ..rect
            })
        }))
    };
    let inside = origin - node.scroll;
    for child in &node.children {
        if child.element.anchor.is_some() {
            if let Some(corner) = child.corner() {
                collect(child, corner, None, visit);
            }
        } else {
            collect(child, inside, clip, visit);
        }
    }
}
