// Generated from the card store by `just card header`: notes go below the end marker.
//! High Tide {U}
//! Instant
//!
//! Until end of turn, whenever a player taps an Island for mana, that player adds an additional
//! {U}.
// End of the generated card header.

use crate::prelude::*;

static EXTRA_BLUE: ManaChange = ManaChange::Adds {
    tapped: Tapped::A(Filter::Subtype(Subtype::Island)),
    added: Added::Of(ManaSet::one(ManaColor::Blue)),
};

static EFFECT: Effect = change_mana(&EXTRA_BLUE, Duration::EndOfTurn);

pub static CARD: CardDef = Printed::HighTide
    .abilities(&[Ability::spell().effect(&EFFECT)])
    .card();
