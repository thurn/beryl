//! What the pointer is over and holding, as the widgets draw it, and the sliders it drags
//! (docs/beryl-3d.md, "Elements"). `Client::point` tells the layer which element a press under
//! the pointer would reach and which one a press went down on, and asks it for the slider under
//! the pointer and, while that slider is dragged, its box.

use glam::Vec2;

use super::{Layer, Node, write::moved};
use crate::element::{Action, Display};
use crate::input::Rect;
use kit_base::geometry::plane::Zoom;
use kit_frame::frame::{ElementId, Role};

impl Layer {
    /// Takes `hovered`, the element a press under the pointer would reach, and `pressed`, the
    /// element a press that has not ended went down on, which the next frame draws so and
    /// marks `HOVERED` and `HELD`.
    pub fn point_at(&mut self, hovered: Option<ElementId>, pressed: Option<ElementId>) {
        self.hovered = hovered;
        self.pressed = pressed;
    }

    /// The enabled slider at `point`, leaving out what waits for the table unless the layer is `open`: its id, its press and its box
    /// on the window.
    #[must_use]
    pub fn slider_at(&self, point: Vec2, open: bool) -> Option<(ElementId, Action, Rect)> {
        self.hit(point, open, |node, rect| {
            slider(node).map(|press| (node.id, press, rect))
        })
    }

    /// The press and the box on the window of the enabled slider `id`, if the screen shows it.
    #[must_use]
    pub fn slider(&self, id: ElementId) -> Option<(Action, Rect)> {
        /// `id`'s node in `node`'s tree, whose top left corner stands at `origin`, with the
        /// corner of the tree it is laid out in.
        fn search(node: &Node, id: ElementId, origin: Vec2, zoom: Zoom) -> Option<(&Node, Rect)> {
            if node.element.style.display == Display::None {
                return None;
            }
            let origin = origin + node.translation();
            let rect = moved(node.paint_rect(), origin + node.offset());
            let zoom = node.zoom(zoom, rect);
            if node.id == id {
                return Some((node, zoom.rect(rect)));
            }
            let inside = origin - node.scroll;
            node.children
                .iter()
                .find_map(|child| match child.element.anchor {
                    Some(_) => search(child, id, child.corner()?, zoom),
                    None => search(child, id, inside, zoom),
                })
        }
        let screen = self.screen.as_ref()?;
        let origin = match screen.element.anchor {
            Some(_) => screen.corner()?,
            None => Vec2::ZERO,
        };
        let (node, rect) = search(screen, id, origin, Zoom::IDENTITY)?;
        slider(node).map(|press| (press, rect))
    }

    /// The press of the focused element, if it is an enabled slider.
    #[must_use]
    pub fn focused_slider(&self) -> Option<Action> {
        self.slider(self.focused?).map(|(press, _)| press)
    }
}

/// The press of `node` if it is an enabled slider.
fn slider(node: &Node) -> Option<Action> {
    let element = &node.element;
    (element.role == Role::Slider && element.enabled)
        .then(|| element.press.clone())
        .flatten()
}
