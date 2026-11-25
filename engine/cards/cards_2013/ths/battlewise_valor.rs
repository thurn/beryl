// Generated from the card store by `just card header`: notes go below the end marker.
//! Battlewise Valor {1}{W}
//! Instant
//!
//! Target creature gets +2/+2 until end of turn. Scry 1. (Look at the top card of your library. You
//! may put that card on the bottom.)
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::BattlewiseValor
    .abilities(&[Ability::spell()
        .target(&TargetSpec::creature())
        .effect(&sequence(&[
            modify(
                target(),
                &[Modification::ModifyPt(2, 2)],
                Duration::EndOfTurn,
            ),
            scry(you(), 1),
        ]))])
    .card();
