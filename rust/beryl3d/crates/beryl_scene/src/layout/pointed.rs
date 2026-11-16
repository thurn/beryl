//! What the pointer is over and holding, as the widgets draw it, and the sliders it drags
//! (docs/beryl-3d.md, "Elements"). `Client::point` tells the layer which element a press under
//! the pointer would reach and which one a press went down on, and asks it for the slider under
//! the pointer and, while that slider is dragged, its box.

use glam::Vec2;

use super::{Layer, Node, write::moved};
use crate::element::{Display, Press};
use crate::frame::{ElementId, Role};
use crate::input::Rect;

impl Layer {
    /// Takes `hovered`, the element a press under the pointer would reach, and `pressed`, the
    /// element a press that has not ended went down on, which the next frame draws so and
    /// marks `HOVERED` and `HELD`.
    pub fn point_at(&mut self, hovered: Option<ElementId>, pressed: Option<ElementId>) {
        self.hovered = hovered;
        self.pressed = pressed;
    }

    /// The enabled slider at `point`, if the screen is `open`: its id, its press and its box
    /// on the window.
    #[must_use]
    pub fn slider_at(&self, point: Vec2, open: bool) -> Option<(ElementId, Press, Rect)> {
        self.hit(point, open, |node, origin| {
            slider(node).map(|press| (node.id, press, node.rect_at(origin)))
        })
    }

    /// The press and the box on the window of the enabled slider `id`, if the screen shows it.
    #[must_use]
    pub fn slider(&self, id: ElementId) -> Option<(Press, Rect)> {
        /// `id`'s node in `node`'s tree, whose top left corner stands at `origin`, with the
        /// corner of the tree it is laid out in.
        fn search(node: &Node, id: ElementId, origin: Vec2) -> Option<(&Node, Vec2)> {
            if node.element.style.display == Display::None {
                return None;
            }
            if node.id == id {
                return Some((node, origin));
            }
            let inside = origin - node.scroll;
            node.children
                .iter()
                .find_map(|child| match child.element.anchor {
                    Some(_) => search(child, id, child.corner()?),
                    None => search(child, id, inside),
                })
        }
        let screen = self.screen.as_ref()?;
        let origin = match screen.element.anchor {
            Some(_) => screen.corner()?,
            None => Vec2::ZERO,
        };
        let (node, origin) = search(screen, id, origin)?;
        slider(node).map(|press| (press, node.rect_at(origin)))
    }

    /// The press of the focused element, if it is an enabled slider.
    #[must_use]
    pub fn focused_slider(&self) -> Option<Press> {
        self.slider(self.focused?).map(|(press, _)| press)
    }
}

/// The press of `node` if it is an enabled slider.
fn slider(node: &Node) -> Option<Press> {
    let element = &node.element;
    (element.role == Role::Slider && element.enabled)
        .then(|| element.press.clone())
        .flatten()
}

impl Node {
    /// Its box on the window, with its tree's top left corner at `origin`, moved by its
    /// transition.
    pub(super) fn rect_at(&self, origin: Vec2) -> Rect {
        moved(self.rect, origin + self.offset())
    }
}
