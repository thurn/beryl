//! Anchored elements (docs/beryl-3d.md, "Elements"): an element with an `anchor` is laid out
//! at its content's size outside its parent's flow, as a taffy tree of its own, and drawn with
//! a chosen point of its box at a point of the window, at a scene object's centre, or above the
//! object's box. The object's place is read on every frame from where the table drew it
//! (`Layer::follow`), so an element anchored to a card stays on the card while it flies, with no
//! layout and no allocation.

use beryl_protocol::script::Subject;
use glam::Vec2;

use super::{Layer, Node};
use crate::element::Anchor;

impl Layer {
    /// Reads where each element anchored to a scene object stands now: `spot` gives the
    /// object's centre and the top of its box on the window, in logical points, or `None` when
    /// the table does not show it, and then the element is not drawn and takes no presses. The
    /// client calls it on every frame, after the table is drawn. Lays out nothing and allocates
    /// nothing.
    pub fn follow(&mut self, spot: impl Fn(Subject) -> Option<Spot>) {
        fn visit(node: &mut Node, spot: &impl Fn(Subject) -> Option<Spot>) {
            match node.element.anchor {
                Some(Anchor::Object(subject, _)) => {
                    node.followed = spot(subject).map(|spot| spot.centre);
                }
                Some(Anchor::Above(subject, _)) => {
                    node.followed = spot(subject).map(|spot| spot.top);
                }
                Some(Anchor::Below(subject, _)) => {
                    node.followed = spot(subject).map(|spot| spot.bottom);
                }
                _ => {}
            }
            for child in &mut node.children {
                visit(child, spot);
            }
        }
        let floats = self.floats.iter_mut().map(|float| &mut float.node);
        for node in self.screen.iter_mut().chain(floats) {
            visit(node, &spot);
        }
    }
}

impl Node {
    /// Where the top left corner of this anchored node's box stands on the window, or `None`
    /// when it is anchored to an object the table does not show, or is a submenu or a tooltip
    /// not shown now. A node that is not anchored
    /// has no corner of its own: it stands where its parent's layout put it.
    pub(super) fn corner(&self) -> Option<Vec2> {
        let rect = self.paint_rect();
        let size = Vec2::new(rect.width, rect.height);
        match self.element.anchor? {
            Anchor::Point(point, at) => Some(point - at * size),
            Anchor::Object(_, at) => self.followed.map(|centre| centre - at * size),
            Anchor::Above(_, gap) => {
                (self.followed).map(|top| top - Vec2::new(size.x / 2.0, size.y + gap))
            }
            Anchor::Below(_, gap) => {
                (self.followed).map(|bottom| bottom + Vec2::new(-size.x / 2.0, gap))
            }
            Anchor::Menu(_) => self.placed,
            Anchor::Submenu => self.placed.filter(|_| self.shown),
            Anchor::Tip => None,
        }
    }
}

/// Where a scene object stands on the window, in logical points: the centre of its box, the
/// middle of the box's highest edge (the highest point of the box, at its centre's x) and the
/// middle of its lowest edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    pub centre: Vec2,
    pub top: Vec2,
    pub bottom: Vec2,
}

impl Spot {
    /// A spot with no box, such as a player's life total's place, whose top and bottom are its
    /// centre.
    #[must_use]
    pub fn point(at: Vec2) -> Self {
        Self {
            centre: at,
            top: at,
            bottom: at,
        }
    }
}
