// Generated from the card store by `just card header`: notes go below the end marker.
//! Diminish {U}
//! Instant
//!
//! Target creature has base power and toughness 1/1 until end of turn.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::Diminish
    .abilities(&[Ability::spell()
        .target(&TargetSpec::creature())
        .effect(&modify(
            target(),
            &[Modification::SetBasePt(
                &Amount::Fixed(1),
                &Amount::Fixed(1),
            )],
            Duration::EndOfTurn,
        ))])
    .card();
