// Generated from the card store by `just card header`: notes go below the end marker.
//! Wild Growth {G}
//! Enchantment — Aura
//!
//! Enchant land
//! Whenever enchanted land is tapped for mana, its controller adds an additional {G}.
// End of the generated card header.

use crate::prelude::*;

static EXTRA_GREEN: ManaChange = ManaChange::Adds {
    tapped: Tapped::AttachedTo,
    added: Added::Of(ManaSet::one(ManaColor::Green)),
};

pub static CARD: CardDef = Printed::WildGrowth
    .abilities(&[
        Ability::spell().target(&TargetSpec::permanent(Filter::Types(Types::LAND))),
        Ability::mana_change(&EXTRA_GREEN),
    ])
    .card();
