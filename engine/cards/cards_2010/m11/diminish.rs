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

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        // An until-end-of-turn effect ends with the turn, so the
        // creature is its printed 2/1 again in the next turn.
        Scenario::new("Diminish's base 1/1 ends with the turn")
            .at(1, P0, TurnStep::PrecombatMain)
            .hand(P0, &[Card::Diminish])
            .battlefield(P0, &[Card::Island, Prop::Vanilla])
            .library(P0, &[Card::Plains, Card::Plains])
            .library(P1, &[Card::Plains, Card::Plains])
            .actions(&[
                ME.casts(Card::Diminish),
                Resolve,
                ME.pt(Prop::Vanilla, 1, 1),
                ME.passes(),
                ME.done_attacking(),
                advance_to(2, TurnStep::PrecombatMain),
                ME.pt(Prop::Vanilla, 2, 1),
            ]),
    ];
}
