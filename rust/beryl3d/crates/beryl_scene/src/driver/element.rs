//! An element of the present frame as the test driver's `find` reports it.

use beryl_protocol::protocol::ZoneName;

use crate::frame::{Place, Role, StateBits};
use crate::input::Rect;

/// An element as `find` found it in the present frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    /// Its index in the frame's resolved elements.
    pub index: u32,
    pub role: Role,
    pub name: String,
    pub place: Place,
    pub rect: Rect,
    pub interactive: bool,
    pub state: StateBits,
}

impl Element {
    /// The zone a card is in; `None` for an element of the flat layer or a face in the Revealed
    /// Cards pile.
    #[must_use]
    pub fn zone(&self) -> Option<ZoneName> {
        match self.place {
            Place::Card { zone, .. } => Some(zone),
            Place::Layer | Place::Revealed => None,
        }
    }

    /// Its bounding box on screen, in logical points.
    #[must_use]
    pub fn rect(&self) -> Rect {
        self.rect
    }

    #[must_use]
    pub fn states(&self) -> StateBits {
        self.state
    }
}
