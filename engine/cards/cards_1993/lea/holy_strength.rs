// Generated from the card store by `just card header`: notes go below the end marker.
//! Holy Strength {W}
//! Enchantment — Aura
//!
//! Enchant creature
//! Enchanted creature gets +1/+2.
// End of the generated card header.

use crate::prelude::*;

/// Implemented for scenarios: a white Aura whose enchanted creature stays a creature, which
/// Mother of Runes's protection from white makes fall off.
pub static CARD: CardDef = Printed::HolyStrength
    .abilities(&[
        Ability::spell().target(&TargetSpec::creature()),
        Ability::static_ability(&LayerSpec {
            affected: Objects::AttachedTo,
            condition: None,
            modifications: &[Modification::ModifyPt(1, 2)],
        }),
    ])
    .card();
