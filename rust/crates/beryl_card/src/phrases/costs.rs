use crate::{Cost, CostPart};

/// "{T}, Sacrifice this land"
pub const TAP_AND_SACRIFICE: Cost = Cost::of_parts(&[CostPart::TAP, CostPart::SACRIFICE]);

/// "Discard your hand, Sacrifice this artifact"
pub const DISCARD_HAND_AND_SACRIFICE: Cost =
    Cost::of_parts(&[CostPart::DiscardHand, CostPart::SACRIFICE]);
