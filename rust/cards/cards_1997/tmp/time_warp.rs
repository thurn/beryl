// Generated from the card store by `just card header`: notes go below the end marker.
//! Time Warp {3}{U}{U}
//! Sorcery
//!
//! Target player takes an extra turn after this one.
// End of the generated card header.

use crate::prelude::*;

/// "Target player takes an extra turn after this one." The turn is added directly after the
/// current one ([CR 500.7]): "Some effects can give a player extra turns. They do this by
/// adding the turns directly after the specified turn." Several extra turns created in one turn
/// are taken in the reverse of the order they were created ([CR 500.7]: "The most recently
/// created turn will be taken first.").
///
/// [CR 500.7]: https://yawgatog.com/resources/magic-rules/#R5007
pub static CARD: CardDef = Printed::TimeWarp
    .abilities(&[Ability::spell()
        .target(&TargetSpec::player())
        .effect(&extra_turn(target_player()))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const WARP: &str = Card::TimeWarp;

    /// P0 in their first main phase with Time Warp and five Islands to cast it, each library
    /// three Plains, so that who draws in each draw step shows whose turn it is.
    const fn warping(name: &'static str) -> Scenario {
        Scenario::new(name)
            .at(1, P0, TurnStep::PrecombatMain)
            .hand(P0, &[WARP])
            .battlefield(P0, &[Card::Island; 5])
            .library(P0, &[Card::Plains; 3])
            .library(P1, &[Card::Plains; 3])
    }

    pub const SCENARIOS: &[Scenario] = &[
        // CR 500.7: P0's extra turn is turn 2, and P1's turn comes after it.
        warping("Time Warp on yourself").actions(&[
            ME.casts(WARP),
            ME.targets_player(ME),
            Resolve,
            ME.has(Zone::Graveyard, WARP, 1),
            advance_to(2, TurnStep::PrecombatMain),
            ME.has(Zone::Hand, Card::Plains, 1),
            THEM.has(Zone::Hand, Card::Plains, 0),
            advance_to(3, TurnStep::PrecombatMain),
            ME.has(Zone::Hand, Card::Plains, 1),
            THEM.has(Zone::Hand, Card::Plains, 1),
        ]),
        // CR 500.7: the extra turn is added directly after P0's turn, so P1 takes turns 2 and
        // 3 in a row, and P0's next turn is turn 4.
        warping("Time Warp on the opponent").actions(&[
            ME.casts(WARP),
            ME.targets_player(THEM),
            Resolve,
            advance_to(3, TurnStep::PrecombatMain),
            THEM.has(Zone::Hand, Card::Plains, 2),
            ME.has(Zone::Hand, Card::Plains, 0),
            advance_to(4, TurnStep::PrecombatMain),
            ME.has(Zone::Hand, Card::Plains, 1),
        ]),
    ];
}
