//! The half of a flip card that a copy reads.
//!
//! Implements: CR 710.2

use beryl_card::FaceIx;
use beryl_card::definition::Shape;

use super::Copiable;
use crate::layout::Layout;
use crate::table::GameTable;

impl<L: Layout> Copiable<L> {
    /// Selects the half a flip card's copiable values read, by the copy's own flipped
    /// status, since status is not copied (see [CR 707.2]). [CR 710.2]: "Once a permanent
    /// is flipped, its normal name, text box, type line, power, and toughness don’t apply
    /// and the alternative versions of those characteristics apply instead."
    ///
    /// [CR 710.2]: https://yawgatog.com/resources/magic-rules/#R7102
    /// [CR 707.2]: https://yawgatog.com/resources/magic-rules/#R7072
    #[must_use]
    pub fn with_flipped_status(self, t: &GameTable, flipped: bool) -> Self {
        if self.face != FaceIx::DOWN && self.shape(t) == Shape::Flip {
            self.turned(t, if flipped { FaceIx::BACK } else { FaceIx::FRONT })
        } else {
            self
        }
    }
}
