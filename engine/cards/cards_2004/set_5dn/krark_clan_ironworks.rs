// Generated from the card store by `just card header`: notes go below the end marker.
//! Krark-Clan Ironworks {4}
//! Artifact
//!
//! Sacrifice an artifact: Add {C}{C}.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::KrarkClanIronworks
    .abilities(&[Ability::mana(
        Cost::of_parts(&[CostPart::sacrifice(Filter::Types(Types::ARTIFACT))]),
        ManaSet::of(ManaColor::Colorless, 2),
    )])
    .card();
