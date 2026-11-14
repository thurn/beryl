// Generated from the card store by `just card header`: notes go below the end marker.
//! Braingeyser {X}{U}{U}
//! Sorcery
//!
//! Target player draws X cards.
// End of the generated card header.

use crate::prelude::*;

/// "Target player draws X cards." The caster announces X while casting ([CR 107.3a]): "the
/// controller of that spell or ability chooses and announces the value of X as part of casting
/// the spell or activating the ability." The announced value is the X in the effect, as the spell
/// is on the stack: here X appears in the text but is defined by the mana cost.
///
/// [CR 107.3a]: https://yawgatog.com/resources/magic-rules/#R1073a
pub static CARD: CardDef = Printed::Braingeyser
    .abilities(&[Ability::spell()
        .target(&TargetSpec::player())
        .effect(&Effect::Draw {
            player: target_player(),
            count: x(),
        })])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const GEYSER: &str = Card::Braingeyser;

    /// {X}{U}{U} with X = 2 needs four lands.
    const LANDS: &[&str] = &[Card::Island; 4];

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Braingeyser for 2 on yourself")
            .hand(P0, &[GEYSER])
            .battlefield(P0, LANDS)
            .library(P0, &[Card::Plains; 4])
            .actions(&[
                ME.casts(GEYSER),
                ME.x(2),
                ME.targets_player(ME),
                Resolve,
                ME.has(Zone::Hand, Card::Plains, 2),
            ]),
        Scenario::new("Braingeyser for 0 draws nothing")
            .hand(P0, &[GEYSER])
            .battlefield(P0, LANDS)
            .library(P0, &[Card::Plains; 4])
            .actions(&[
                ME.casts(GEYSER),
                ME.x(0),
                ME.targets_player(ME),
                Resolve,
                ME.has(Zone::Hand, Card::Plains, 0),
            ]),
        Scenario::new("Braingeyser on the opponent")
            .hand(P0, &[GEYSER])
            .battlefield(P0, LANDS)
            .library(P1, &[Card::Plains; 4])
            .actions(&[
                ME.casts(GEYSER),
                ME.x(2),
                ME.targets_player(THEM),
                Resolve,
                THEM.has(Zone::Hand, Card::Plains, 2),
            ]),
        // Drawing more than the library holds draws what is there, and the player loses at the
        // next state-based action check ([CR 704.5b]).
        //
        // [CR 704.5b]: https://yawgatog.com/resources/magic-rules/#R7045b
        Scenario::new("Braingeyser for more than the library")
            .hand(P0, &[GEYSER])
            .battlefield(P0, LANDS)
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.casts(GEYSER),
                ME.x(2),
                ME.targets_player(THEM),
                Resolve,
                THEM.has(Zone::Hand, Card::Plains, 1),
                wins(ME),
            ]),
    ];
}
