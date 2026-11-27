// Generated from the card store by `just card header`: notes go below the end marker.
//! Muscle Sliver {1}{G}
//! Creature — Sliver, 1/1
//!
//! All Sliver creatures get +1/+1.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::MuscleSliver
    .abilities(&[Ability::static_ability(&LayerSpec {
        affected: permanents(Filter::All(&[creature(), Filter::Subtype(Subtype::Sliver)])),
        condition: None,
        modifications: &[Modification::ModifyPt(1, 1)],
    })])
    .card();
