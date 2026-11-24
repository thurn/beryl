// Generated from the card store by `just card header`: notes go below the end marker.
//! Builder's Blessing {3}{W}
//! Enchantment
//!
//! Untapped creatures you control get +0/+2.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::BuildersBlessing
    .abilities(&[Ability::static_ability(&LayerSpec {
        affected: Objects::All {
            zone: Zone::Battlefield,
            filter: Filter::All(&[
                Filter::Types(Types::CREATURE),
                Filter::Whose(&Players::You),
                Filter::Not(&Filter::Tapped),
            ]),
        },
        condition: None,
        modifications: &[Modification::ModifyPt(0, 2)],
    })])
    .card();
