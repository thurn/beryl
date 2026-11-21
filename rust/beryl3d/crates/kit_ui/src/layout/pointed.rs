//! What the pointer is over and holding, as the widgets draw it, and the sliders it drags
//! (docs/beryl-3d.md, "Elements"). `Client::point` tells the layer which element a press under
//! the pointer would reach and which one a press went down on, and asks it for the slider under
//! the pointer and, while that slider is dragged, its box.

use glam::Vec2;

use super::{Layer, Node, write::moved};
use crate::element::{Action, Overflow};
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

    /// The first value `want` gives for an element at `point`, asking the innermost first:
    /// the floats drawn over the screen, the last drawn first, then the screen, without what
    /// waits for the table to settle (`Element::waits`) unless the layer is `open`, then the
    /// floats drawn under it. A visible modal overlay owns the input. An element is at
    /// `point` where it is drawn: moved by its own transition and by the scroll of the elements
    /// it is in, and inside the box of every element it is in that clips or scrolls. As in CSS,
    /// a child that overflows a box that does neither is drawn outside it, and takes presses
    /// there; `write` marks the same elements interactive.
    pub(super) fn hit<T>(
        &self,
        point: glam::Vec2,
        open: bool,
        want: impl Fn(&Node, Rect) -> Option<T>,
    ) -> Option<T> {
        use kit_frame::frame::picking::{Area, hit_test};

        // Visit in reverse layout tree order. An anchored child starts its own clip chain.
        fn visit<'a, T>(
            node: &'a Node,
            (origin, open, zoom): (glam::Vec2, bool, Zoom),
            clip: Option<Rect>,
            test: &dyn Fn(Area, (&'a Node, Rect)) -> Option<T>,
        ) -> Option<T> {
            if !node.accepts_input(open) {
                return None;
            }
            let origin = origin + node.translation();
            let rect = moved(node.paint_rect(), origin + node.offset());
            let zoom = node.zoom(zoom, rect);
            let rect = zoom.rect(rect);
            let inside = origin - node.scroll;
            let clips = node.element.style.overflow != Overflow::Visible;
            let inside_clip = if clips {
                Some(Area::clip(rect, clip))
            } else {
                clip
            };
            (node.children.iter().rev())
                .find_map(|child| match child.element.anchor {
                    // An anchored child's tree is laid out from (0, 0), and stands at its corner.
                    Some(_) => visit(child, (child.corner()?, open, zoom), None, test),
                    None => visit(child, (inside, open, zoom), inside_clip, test),
                })
                .or_else(|| test(Area::Rect(rect, clip), (node, rect)))
        }
        hit_test(
            point,
            |test| {
                if let Some(modal) = self.input_modal(open) {
                    return visit(modal, (modal.corner()?, open, Zoom::IDENTITY), None, test);
                }
                let floats = |under: bool| {
                    (self.floats.iter().rev())
                        .filter(move |float| float.id.under_screen() == under)
                        .find_map(|float| {
                            let corner = float.corner()?;
                            let rect = float.node.rect;
                            let pivot = corner + glam::Vec2::new(rect.width, rect.height) / 2.0;
                            let zoom = Zoom::around(pivot, float.look.scale);
                            visit(&float.node, (corner, open, zoom), None, test)
                        })
                };
                floats(false)
                    .or_else(|| {
                        (self.screen.as_ref()).and_then(|node| match node.element.anchor {
                            Some(_) => {
                                visit(node, (node.corner()?, open, Zoom::IDENTITY), None, test)
                            }
                            None => {
                                visit(node, (glam::Vec2::ZERO, open, Zoom::IDENTITY), None, test)
                            }
                        })
                    })
                    .or_else(|| floats(true))
            },
            |(node, rect)| want(node, rect),
        )
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
        let (node, rect) = self.positioned_node(id, true)?;
        slider(node).map(|press| (press, rect))
    }

    /// The live box of an active screen node, independent of its paint order.
    pub(super) fn positioned_node(&self, id: ElementId, open: bool) -> Option<(&Node, Rect)> {
        fn search<'a>(
            layer: &Layer,
            node: &'a Node,
            id: ElementId,
            open: bool,
            origin: Vec2,
            zoom: Zoom,
        ) -> Option<(&'a Node, Rect)> {
            if !node.accepts_input(open) {
                return None;
            }
            let origin = origin + node.translation();
            let sink = layer.node_sink(node, layer.state(node));
            let rect = moved(node.paint_rect(), origin + node.offset() + sink);
            let zoom = node.zoom(zoom, rect);
            if node.id == id {
                return Some((node, zoom.rect(rect)));
            }
            let inside = origin - node.scroll;
            node.children.iter().find_map(|child| {
                let origin = match child.element.anchor {
                    Some(_) => child.corner()?,
                    None => inside,
                };
                search(layer, child, id, open, origin, zoom)
            })
        }
        let screen = self.screen.as_ref()?;
        let origin = match screen.element.anchor {
            Some(_) => screen.corner()?,
            None => Vec2::ZERO,
        };
        search(self, screen, id, open, origin, Zoom::IDENTITY)
    }

    /// The press of the focused element, if it is an enabled slider.
    #[must_use]
    pub fn focused_slider(&self, open: bool) -> Option<Action> {
        self.focused_node(open).and_then(slider)
    }
}

/// The press of `node` if it is an enabled slider.
fn slider(node: &Node) -> Option<Action> {
    let element = &node.element;
    (element.role == Role::Slider && element.enabled)
        .then(|| element.press.clone())
        .flatten()
}
