// Generated from the card store by `just card header`: notes go below the end marker.
//! Angel's Feather {2}
//! Artifact
//!
//! Whenever a player casts a white spell, you may gain 1 life.
// End of the generated card header.

use crate::prelude::*;

/// Its controller may gain life when either player casts a white spell. Implemented for
/// color-changing scenarios.
pub static CARD: CardDef = Printed::AngelsFeather
    .abilities(&[Ability::triggered(&Trigger::casts(
        Players::EachPlayer,
        Filter::Colors(Colors::WHITE),
    ))
    .effect(&may(you(), &gain_life(you(), 1)))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] =
        &[
            Scenario::new("Angel's Feather can gain life when an opponent casts a white spell")
                .battlefield(P0, &[Card::AngelsFeather])
                .battlefield(P1, &[Card::Plains; 4])
                .hand(P1, &[Prop::Flier])
                .at(2, P1, TurnStep::PrecombatMain)
                .actions(&[
                    THEM.casts(Prop::Flier),
                    stack(2),
                    Resolve,
                    ME.yes(),
                    ME.life(21),
                    THEM.controls(Prop::Flier, 0),
                    Resolve,
                    THEM.controls(Prop::Flier, 1),
                ]),
        ];
}
