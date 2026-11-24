//! Optional solver decisions read by the inspection driver.

use beryl_protocol::protocol::{RowName, Side, ZoneName};

use super::{Placement, RowDecision, row_index, side_index};
use kit_space::arrange::Overflow;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Decisions {
    pub(super) battlefield: [[RowDecision; 3]; 2],
    pub hand: [Option<ContainerDecision>; 2],
    pub stack: Option<ContainerDecision>,
}

/// What the hand or stack arrangement solver chose for its region and target size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContainerDecision {
    pub items: usize,
    pub overflow: Overflow,
    pub press_points: f32,
}

impl Placement {
    /// Row fit recorded by an inspection solve.
    #[must_use]
    pub fn row_decision(&self, side: Side, row: RowName) -> Option<RowDecision> {
        Some(self.decisions.as_ref()?.battlefield[side_index(side)][row_index(row)])
    }

    /// The decision for one of the two fans or the shared stack.
    #[must_use]
    pub fn container_decision(&self, zone: ZoneName, side: Side) -> Option<ContainerDecision> {
        let decisions = self.decisions.as_ref()?;
        match zone {
            ZoneName::Hand => decisions.hand[side_index(side)],
            ZoneName::Stack => decisions.stack,
            _ => None,
        }
    }
}
