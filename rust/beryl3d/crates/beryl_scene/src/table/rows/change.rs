//! Which object changes affect the layout of its battlefield row.

use beryl_protocol::protocol::{ObjectView, ZoneName};

use super::{plain, same_pile_face};

/// What an in-place record update may change about the placement of a permanent in its row.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowChange {
    Layout,
    Piling,
    Unchanged,
}

impl RowChange {
    pub(crate) fn between(was: &ObjectView, now: &ObjectView) -> Self {
        if was.zone != ZoneName::Battlefield
            || now.zone != ZoneName::Battlefield
            || was.attached_to.is_some()
            || now.attached_to.is_some()
            || was.types != now.types
            || was.tapped != now.tapped
            || was.name != now.name
            || was.facing != now.facing
            || was.phased_out != now.phased_out
        {
            Self::Layout
        } else {
            match (plain(was), plain(now)) {
                (false, false) => Self::Unchanged,
                (true, true) if same_pile_face(was, now) => Self::Unchanged,
                _ => Self::Piling,
            }
        }
    }
}
