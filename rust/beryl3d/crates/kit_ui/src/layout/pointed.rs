//! What the pointer is over and holding, as the widgets draw it, and the sliders it drags
//! (docs/beryl-3d.md, "Elements"). `Client::point` tells the layer which element a press under
//! the pointer would reach and which one a press went down on, and asks it for the slider under
//! the pointer and, while that slider is dragged, its box.

use glam::Vec2;

use super::{
    Layer, Node, element,
    write::{self, moved},
};
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

    /// The first value `want` gives for an element at `point`, asking the innermost first:
    /// the floats drawn over the screen, the last drawn first, then the screen, without what
    /// waits for the table to settle (`Element::waits`) unless the layer is `open`, then the
    /// floats drawn under it. While the screen shows
    /// a child drawn over the floats (`write::over_floats`), only that child is asked. An element is at
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
            if node.element.style.display == element::Display::None || (node.element.waits && !open)
            {
                return None;
            }
            let origin = origin + node.translation();
            let rect = moved(node.paint_rect(), origin + node.offset());
            let zoom = node.zoom(zoom, rect);
            let rect = zoom.rect(rect);
            let inside = origin - node.scroll;
            let clips = node.element.style.overflow != element::Overflow::Visible;
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
                // The screen's children drawn over the floats (`write::over_floats`) filter what is under
                // them, and are modal: while one is shown, nothing else is searched.
                let modal_child =
                    |child: &&Node| write::over_floats(child) && (open || !child.element.waits);
                let modal = (self.screen.as_ref())
                    .filter(|node| node.children.iter().any(|child| modal_child(&child)));
                if let Some(node) = modal {
                    return (node.children.iter().rev())
                        .filter(modal_child)
                        .find_map(|child| {
                            visit(child, (child.corner()?, open, Zoom::IDENTITY), None, test)
                        });
                }
                let floats = |under: bool| {
                    (self.floats.iter().rev())
                        .filter(move |float| float.id.under_screen() == under)
                        .find_map(|float| {
                            let corner = float.corner();
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
