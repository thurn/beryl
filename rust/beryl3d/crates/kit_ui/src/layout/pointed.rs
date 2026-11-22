//! What the pointer is over and holding, as the widgets draw it, and the sliders it drags
//! (docs/beryl-3d.md, "Elements"). `Client::point` tells the layer which element a press under
//! the pointer would reach and which one a press went down on, and asks it for the slider under
//! the pointer and, while that slider is dragged, its box.

use glam::Vec2;

use super::{Layer, Node};
use crate::element::{Action, Anchor};
use crate::input::Rect;
use crate::scroll::Viewport;
use kit_base::geometry::plane::Zoom;
use kit_frame::frame::picking::{Area, hit_test};
use kit_frame::frame::{ElementId, Role, ScreenClip};

/// A live screen node and the input affordances of its containing branch.
pub struct PointerTarget {
    pub id: ElementId,
    pub role: Role,
    pub press: Option<(Action, bool)>,
    pub shown: Option<ElementId>,
    pub focus: bool,
    pub slider: Option<Rect>,
    pub scroller: Option<(ElementId, Viewport)>,
    pub menu: bool,
    pub tooltip: Option<ElementId>,
}

#[derive(Clone, Copy, Default)]
struct Branch {
    scroller: Option<(ElementId, Viewport)>,
    menu: bool,
    passes: bool,
    control: bool,
}

struct Pick<'a, 'test, T> {
    layer: &'a Layer,
    open: bool,
    wheel: Option<(Vec2, f32)>,
    test: &'test dyn Fn(Area, (&'a Node, Rect, Branch)) -> Option<T>,
}

impl<'a, T> Pick<'a, '_, T> {
    fn visit(
        &self,
        node: &'a Node,
        (origin, zoom): (Vec2, Zoom),
        clip: Option<ScreenClip>,
        branch: Branch,
        children: impl Iterator<Item = &'a Node>,
    ) -> Option<T> {
        if !node.accepts_input(self.open) {
            return None;
        }
        let geometry = self
            .layer
            .geometry(node, origin, zoom, self.layer.state(node));
        let masked = geometry.mask(node.mask(), clip);
        let inside_clip = geometry.inside_clip(node, masked, clip);
        let inside = geometry.inside(node);
        let rect = geometry.painted;
        let zoom = geometry.zoom;
        let viewport = node.viewport().filter(|viewport| {
            self.wheel
                .is_none_or(|(delta, notch)| viewport.takes_wheel(delta, notch))
        });
        let ancestor_control = branch.control;
        let branch = Branch {
            control: ancestor_control || node.element.press.is_some() || node.element.focus,
            scroller: viewport
                .map(|viewport| (node.id, viewport))
                .or(branch.scroller),
            menu: branch.menu
                || matches!(node.element.anchor, Some(Anchor::Menu(_) | Anchor::Submenu)),
            passes: branch.passes || node.element.style.passes,
        };
        let mut children = children;
        (children.find_map(|child| match child.element.anchor {
            Some(_) => self.node(
                child,
                (child.corner()?, zoom),
                None,
                Branch {
                    scroller: None,
                    ..branch
                },
            ),
            None => self.node(child, (inside, zoom), inside_clip, branch),
        }))
        .or_else(|| {
            (self.test)(
                Area::Rect(rect, masked.or(clip).map(|clip| clip.bounds)),
                (
                    node,
                    rect,
                    Branch {
                        control: ancestor_control,
                        ..branch
                    },
                ),
            )
        })
    }

    fn node(
        &self,
        node: &'a Node,
        space: (Vec2, Zoom),
        clip: Option<ScreenClip>,
        branch: Branch,
    ) -> Option<T> {
        self.visit(node, space, clip, branch, node.children.iter().rev())
    }
}

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
        self.pick(point, open, None, |node, rect, _| want(node, rect))
    }

    /// Picks the frontmost live input node once. Scroll and menu information follows the
    /// winning node's ancestry, so an overlapping sibling cannot claim the same event.
    #[must_use]
    pub fn pointer_target(
        &self,
        point: Vec2,
        open: bool,
        wheel: Option<(Vec2, f32)>,
    ) -> Option<PointerTarget> {
        // Context menus own presses within their bounds, including over HUD floats.
        let menu_only = open && self.input_modal(open).is_none() && self.menu_open();
        self.pick(point, open, wheel, |node, rect, branch| {
            if menu_only && !branch.menu {
                return None;
            }
            let element = &node.element;
            let tooltip = self.tooltip_of(&element.name);
            let menu = matches!(element.anchor, Some(Anchor::Menu(_) | Anchor::Submenu));
            let takes_input = element.press.is_some() || element.focus || tooltip.is_some();
            let scrolls = branch.scroller.is_some_and(|(id, _)| id == node.id);
            let shown = element.show.map(kit_frame::frame::Show::object_id);
            let panel = !branch.passes
                && !branch.control
                && (element.role == Role::Player
                    || element.style.background.is_some_and(|color| color[3] > 0)
                    || !element.style.backdrop_filter.is_empty());
            if !(takes_input || scrolls || menu || panel || shown.is_some()) {
                return None;
            }
            Some(PointerTarget {
                id: node.id,
                role: element.role,
                press: element.press.clone().map(|press| (press, element.enabled)),
                shown,
                focus: element.focus,
                slider: (element.enabled && element.role == Role::Slider).then_some(rect),
                scroller: branch.scroller,
                menu: branch.menu,
                tooltip,
            })
        })
    }

    /// Whether an overlay owns the space outside its controls too.
    #[must_use]
    pub fn pointer_modal(&self, open: bool) -> bool {
        self.input_modal(open).is_some()
    }

    fn pick<T>(
        &self,
        point: Vec2,
        open: bool,
        wheel: Option<(Vec2, f32)>,
        want: impl Fn(&Node, Rect, Branch) -> Option<T>,
    ) -> Option<T> {
        hit_test(
            point,
            |test| {
                let pick = Pick {
                    layer: self,
                    open,
                    wheel,
                    test,
                };
                let branch = Branch::default();
                if let Some(modal) = self.input_modal(open) {
                    return pick.node(modal, (modal.corner()?, Zoom::IDENTITY), None, branch);
                }
                let floats = |under: bool| {
                    (self.floats.iter().rev())
                        .filter(move |float| float.id.under_screen() == under)
                        .find_map(|float| {
                            let corner = float.corner()?;
                            let rect = float.node.rect;
                            let pivot = corner + Vec2::new(rect.width, rect.height) / 2.0;
                            let zoom = Zoom::around(pivot, float.look.scale);
                            pick.node(&float.node, (corner, zoom), None, branch)
                        })
                };
                let over_floats = self
                    .screen
                    .iter()
                    .flat_map(|node| node.children.iter().rev())
                    .filter(|child| super::write::above_floats(child))
                    .find_map(|child| {
                        pick.node(child, (child.corner()?, Zoom::IDENTITY), None, branch)
                    });
                over_floats
                    .or_else(|| floats(false))
                    .or_else(|| {
                        let node = self.screen.as_ref()?;
                        let corner = match node.element.anchor {
                            Some(_) => node.corner()?,
                            None => Vec2::ZERO,
                        };
                        let children = node
                            .children
                            .iter()
                            .rev()
                            .filter(|child| !super::write::above_floats(child));
                        pick.visit(node, (corner, Zoom::IDENTITY), None, branch, children)
                    })
                    .or_else(|| floats(true))
            },
            |(node, rect, branch)| want(node, rect, branch),
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
            let geometry = layer.geometry(node, origin, zoom, layer.state(node));
            if node.id == id {
                return Some((node, geometry.painted));
            }
            let inside = geometry.inside(node);
            node.children.iter().find_map(|child| {
                let origin = match child.element.anchor {
                    Some(_) => child.corner()?,
                    None => inside,
                };
                search(layer, child, id, open, origin, geometry.zoom)
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
