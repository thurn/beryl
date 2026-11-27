// Generated from the card store by `just card header`: notes go below the end marker.
//! Gemhide Sliver {1}{G}
//! Creature — Sliver, 1/1
//!
//! All Slivers have "{T}: Add one mana of any color."
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::GemhideSliver
    .abilities(&[Ability::static_ability(&LayerSpec {
        affected: Objects::All {
            zone: Zone::Battlefield,
            filter: Filter::Subtype(Subtype::Sliver),
        },
        condition: None,
        modifications: &[Modification::grant(0)],
    })])
    .grants(&[Ability::tap_for_mana(ManaSet::any_color(1))])
    .card();
