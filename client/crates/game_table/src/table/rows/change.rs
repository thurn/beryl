//! Which object changes affect the layout of its battlefield row.

use beryl_protocol::protocol::{Combat, ObjectId, ObjectView, ZoneName};

use super::{plain, same_pile_face};

/// What an in-place record update may change about the placement of a permanent in its row.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RowChange {
    /// The row may lay out differently: the card's width, its place in the row's order or what
    /// lies under it may have changed.
    Layout,
    /// Only whether the card can share a pile with its twins changed.
    Piling,
    /// Whether the card shows its face, or whether it is phased out, changed, and nothing that
    /// decides where it or any other card of the row stands. Unless it could share a pile with
    /// another card of its name, only its own pose changes (`rows::reposed`).
    Pose,
    Unchanged,
}

impl RowChange {
    /// Compares two records to find what changed about the permanent's row placement.
    pub fn between(was: &ObjectView, now: &ObjectView) -> Self {
        if was.zone != ZoneName::Battlefield
            || now.zone != ZoneName::Battlefield
            || was.attached_to.is_some()
            || now.attached_to.is_some()
            || was.types != now.types
            || was.tapped != now.tapped
            || was.name != now.name
            || stance(was) != stance(now)
        {
            Self::Layout
        } else if was.facing != now.facing || was.phased_out != now.phased_out {
            Self::Pose
        } else {
            match (plain(was), plain(now)) {
                (false, false) => Self::Unchanged,
                (true, true) if same_pile_face(was, now) => Self::Unchanged,
                _ => Self::Piling,
            }
        }
    }
}

/// How `object` fights, which decides where it stands in its row: an attacker moves out of its
/// row toward the other side on the desktop (`FittedRow::advance`), and a blocker stands across
/// from the attacker it blocks (`rows::face`).
/// It gives whether it attacks, and the attacker it blocks.
fn stance(object: &ObjectView) -> (bool, Option<ObjectId>) {
    match object.combat {
        Some(Combat::Attacking { .. }) => (true, None),
        Some(Combat::Blocking { attacker }) => (false, attacker),
        None => (false, None),
    }
}
