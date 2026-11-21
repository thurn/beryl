//! The life-orb coordinates shared by the HUD and the effects anchored to the life total.

use beryl_protocol::protocol::Side;

/// The artwork's left, top, width and height, in units relative to its origin.
pub const ART: [f32; 4] = [-136.0, -57.0, 272.0, 116.0];

/// The center of the viewer's core, in artwork units below the origin.
pub const CORE_CY: f32 = -3.0;

/// The offset of the life total from the middle of the orb's artwork, in artwork units.
#[must_use]
pub fn face_offset(side: Side) -> f32 {
    let middle = ART[1] + ART[3] / 2.0;
    match side {
        Side::You => CORE_CY - middle,
        Side::Opponent => -CORE_CY - middle,
    }
}
