//! The flat layer's floats (docs/beryl-3d.md, "Floats"): small elements placed at a point of
//! the window, such as the turn banner, a life total or the count on a pile, outside the flex
//! flow that places the screen. Each float is a taffy tree of its own, laid out at its
//! content's size, so a float never moves the prompt bar. Where it stands, how opaque it is and
//! the color of its text are its look, which can change on every frame without laying anything
//! out.

use glam::Vec2;

use super::{Layer, Node};
use crate::element::{Element, FloatId};

/// Where a float stands and how it is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloatLook {
    /// The point of the window the float stands at, in logical points.
    pub point: Vec2,
    /// Which point of the float's box lies at `point`: (0, 0) is its top left corner, (0.5, 0.5)
    /// its centre and (1, 1) its bottom right corner.
    pub anchor: Vec2,
    /// How opaque every part of the float is, from 0 to 1.
    pub opacity: f32,
    /// The color of the float's text in place of the design tokens' type color, if any.
    pub tint: Option<[u8; 4]>,
}

impl Default for FloatLook {
    /// At the window's top left corner, opaque, with its usual colors.
    fn default() -> Self {
        Self {
            point: Vec2::ZERO,
            anchor: Vec2::ZERO,
            opacity: 1.0,
            tint: None,
        }
    }
}

impl FloatLook {
    /// Where the top left corner of a box of `size` stands under this look.
    pub(super) fn corner(&self, size: Vec2) -> Vec2 {
        self.point - self.anchor * size
    }
}

/// One float: its element tree, laid out with its top left corner at (0, 0), and its look.
#[derive(Debug)]
pub(super) struct Float {
    pub id: FloatId,
    pub node: Node,
    pub look: FloatLook,
}

impl Float {
    /// Where the float's top left corner stands on screen.
    pub fn corner(&self) -> Vec2 {
        let rect = self.node.rect;
        self.look.corner(Vec2::new(rect.width, rect.height))
    }
}

impl Layer {
    /// Shows `element` as the float `id`, or removes that float. The element is matched against
    /// the one the float had, as `show` matches a screen: an equal element keeps its layout and
    /// its laid-out text. A new float takes the default look until `set_float_look` gives it
    /// another; a float that is kept keeps its look.
    pub fn float(&mut self, id: FloatId, element: Option<Element>) {
        let at = self.floats.binary_search_by_key(&id, |float| float.id);
        match (at, element) {
            (Ok(index), Some(element)) => {
                let Float { node, look, .. } = self.floats.remove(index);
                let node = self.reconcile(Some(node), element);
                self.floats.insert(index, Float { id, node, look });
            }
            (Err(index), Some(element)) => {
                let node = self.reconcile(None, element);
                self.floats.insert(
                    index,
                    Float {
                        id,
                        node,
                        look: FloatLook::default(),
                    },
                );
            }
            (Ok(index), None) => {
                let float = self.floats.remove(index);
                self.remove(float.node);
            }
            (Err(_), None) => {}
        }
        self.relayout();
    }

    /// Sets where the float `id` stands and how it is drawn. It lays nothing out and allocates
    /// nothing, so a client may call it on every frame to move or fade a float. A float not
    /// shown has no look to set, and the call does nothing.
    pub fn set_float_look(&mut self, id: FloatId, look: FloatLook) {
        if let Ok(index) = self.floats.binary_search_by_key(&id, |float| float.id) {
            self.floats[index].look = look;
        }
    }
}
