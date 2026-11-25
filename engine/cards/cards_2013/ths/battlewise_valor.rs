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

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        // An until-end-of-turn effect ends with the turn, so the
        // creature is its printed 2/1 again in the next turn.
        Scenario::new("Battlewise Valor's bonus ends with the turn")
            .interaction(&[Card::BattlewiseValor, Card::Plains])
            .at(1, P0, TurnStep::PrecombatMain)
            .hand(P0, &[Card::BattlewiseValor])
            .battlefield(P0, &[Card::Plains, Card::Plains, Prop::Vanilla])
            .library(P0, &[Card::Plains, Card::Plains])
            .library(P1, &[Card::Plains, Card::Plains])
            .actions(&[
                ME.casts(Card::BattlewiseValor),
                Resolve,
                ME.done_picking(),
                ME.pt(Prop::Vanilla, 4, 3),
                ME.done_attacking(),
                advance_to(2, TurnStep::PrecombatMain),
                ME.pt(Prop::Vanilla, 2, 1),
            ]),
    ];
}
