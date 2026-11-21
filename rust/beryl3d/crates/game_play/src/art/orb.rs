//! The game side selects which orientation of the life orb to draw.

use beryl_protocol::protocol::Side;

use kit_ui::orb::{self, Orientation};
use kit_ui::widgets::icons::LayeredIcon;

pub use kit_ui::orb::{ART, BEZEL, CORE_RX, CORE_RY, DESKTOP_SCALE, SCALES};

const fn orientation(side: Side) -> Orientation {
    match side {
        Side::You => Orientation::Upright,
        Side::Opponent => Orientation::Mirrored,
    }
}

/// The art filling an orb's box, lit if its seat is active.
#[must_use]
pub fn artwork(side: Side, active: bool, scale: u8) -> LayeredIcon {
    orb::artwork(orientation(side), active, scale)
}

/// The offset of the core within the art's box, in art units.
#[must_use]
pub fn face_offset(side: Side) -> f32 {
    orb::face_offset(orientation(side))
}
