//! Anchored elements (docs/beryl-3d.md, "Elements"): an element with an `anchor` is laid out
//! at its content's size outside its parent's flow, as a taffy tree of its own, and drawn with
//! a chosen point of its box at a point of the window or at a scene object's centre. The
//! object's centre is read on every frame from where the table drew it (`Layer::follow`), so
//! an element anchored to a card stays on the card while it flies, with no layout and no
//! allocation.

use beryl_protocol::script::Subject;
use glam::Vec2;

use super::{Layer, Node};
use crate::element::Anchor;
use crate::frame::{ElementId, FrameBuilder};
use crate::table::{ABILITY_IDS, Table};

impl Layer {
    /// Reads where each element anchored to a scene object stands now: `centre` gives the
    /// object's centre on the window, in logical points, or `None` when the table does not show
    /// it, and then the element is not drawn and takes no presses. The client calls it on every
    /// frame, after the table is drawn. Lays out nothing and allocates nothing.
    pub fn follow(&mut self, centre: impl Fn(Subject) -> Option<Vec2>) {
        fn visit(node: &mut Node, centre: &impl Fn(Subject) -> Option<Vec2>) {
            if let Some(Anchor::Object(subject, _)) = node.element.anchor {
                node.followed = centre(subject);
            }
            for child in &mut node.children {
                visit(child, centre);
            }
        }
        let floats = self.floats.iter_mut().map(|float| &mut float.node);
        for node in self.screen.iter_mut().chain(floats) {
            visit(node, &centre);
        }
    }
}

impl Node {
    /// Where the top left corner of this anchored node's box stands on the window, or `None`
    /// when it is anchored to an object the table does not show, or is a submenu or a tooltip
    /// not shown now. A node that is not anchored
    /// has no corner of its own: it stands where its parent's layout put it.
    pub(super) fn corner(&self) -> Option<Vec2> {
        let size = Vec2::new(self.rect.width, self.rect.height);
        match self.element.anchor? {
            Anchor::Point(point, at) => Some(point - at * size),
            Anchor::Object(_, at) => self.followed.map(|centre| centre - at * size),
            Anchor::Menu(_) => self.placed,
            Anchor::Submenu | Anchor::Tip => self.placed.filter(|_| self.shown),
        }
    }
}

/// The centre of `subject` on the window as the table drew it into `builder` this frame, in
/// logical points: a card's or an ability's box, wherever its flight has taken it, or a
/// player's life total's place. `None` for a card or an ability the table did not draw.
#[must_use]
pub fn centre_of(builder: &FrameBuilder, table: &Table, subject: Subject) -> Option<Vec2> {
    match subject {
        Subject::Object { slot } => builder.centre(ElementId(u32::from(slot.0))),
        Subject::Ability { id } => builder.centre(ElementId(ABILITY_IDS + id % ABILITY_IDS)),
        Subject::Player { player } => {
            Some(table.camera().to_screen(table.player_pose(player).position))
        }
    }
}
