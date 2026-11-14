// Generated from the card store by `just card header`: notes go below the end marker.
//! Cerebral Vortex {1}{U}{R}
//! Instant
//!
//! Target player draws two cards, then Cerebral Vortex deals damage to that player equal to the
//! number of cards they've drawn this turn.
// End of the generated card header.

use crate::prelude::*;

/// "Target player draws two cards, then Cerebral Vortex deals damage to that player equal to the
/// number of cards they've drawn this turn." The count includes the two cards just drawn
/// ([CR 121.1]): "A player draws a card by putting the top card of their library into their
/// hand." A draw from an empty library puts no card into a hand, so it is not counted. The spell
/// is the source of the damage ([CR 120.2b]): "The spell or ability will specify which object
/// deals that damage."
///
/// [CR 121.1]: https://yawgatog.com/resources/magic-rules/#R1211
/// [CR 120.2b]: https://yawgatog.com/resources/magic-rules/#R1202b
pub static CARD: CardDef = Printed::CerebralVortex
    .abilities(&[Ability::spell()
        .target(&TargetSpec::player())
        .effect(&sequence(&[
            draw(target_player(), 2),
            Effect::DealDamage {
                to: Recipients::Players(target_player()),
                amount: Amount::CardsDrawn(target_player()),
            },
        ]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    /// P0's upkeep of turn 3. The scenarios begin in turn 2, P1's, so that XMage's first turn is
    /// not the one that counts: in it the starting player skips their draw ([CR 103.8a]): "In a
    /// two-player game, the player who plays first skips the draw step (see rule 504, "Draw Step")
    /// of their first turn."
    ///
    /// [CR 103.8a]: https://yawgatog.com/resources/magic-rules/#R1038a
    const TO_UPKEEP: Action = advance_to(3, TurnStep::Upkeep);

    const VORTEX: &str = Card::CerebralVortex;

    /// {1}{U}{R}: an Island, a Mountain and a spare Island.
    const LANDS: &[&str] = &[Card::Island, Card::Island, Card::Mountain];

    pub const SCENARIOS: &[Scenario] = &[
        // On the opponent, before their own draw step has happened: the two cards drawn count,
        // and the opponent's draw step is not P0's.
        Scenario::new("Cerebral Vortex on the opponent: 2 damage")
            .at(2, P1, TurnStep::PostcombatMain)
            .battlefield(P0, LANDS)
            .hand(P0, &[VORTEX])
            .library(P1, &[Card::Plains; 6])
            .actions(&[
                TO_UPKEEP,
                ME.casts(VORTEX),
                ME.targets_player(THEM),
                THEM.life(18),
            ]),
        // Cast in its caster's upkeep, before their draw: only the two cards drawn count.
        Scenario::new("Cerebral Vortex before the draw: 2 damage")
            .at(2, P1, TurnStep::PostcombatMain)
            .battlefield(P0, LANDS)
            .hand(P0, &[VORTEX])
            .library(P0, &[Card::Plains; 6])
            .actions(&[
                TO_UPKEEP,
                ME.casts(VORTEX),
                ME.targets_player(ME),
                ME.life(18),
            ]),
        // After the draw step the card drawn there counts too.
        Scenario::new("Cerebral Vortex after the draw: 3 damage")
            .at(2, P1, TurnStep::PostcombatMain)
            .battlefield(P0, LANDS)
            .hand(P0, &[VORTEX])
            .library(P0, &[Card::Plains; 6])
            .actions(&[
                advance_to(3, TurnStep::PrecombatMain),
                ME.casts(VORTEX),
                ME.targets_player(ME),
                ME.life(17),
            ]),
        // A draw from an empty library puts no card in hand and is not counted: one card is
        // drawn, and one damage dealt. The player then loses for the failed draw
        // ([CR 704.5b](https://yawgatog.com/resources/magic-rules/#R7045b)).
        Scenario::new("Cerebral Vortex with one card in the library")
            .at(2, P1, TurnStep::PostcombatMain)
            .battlefield(P0, LANDS)
            .hand(P0, &[VORTEX])
            .library(P1, &[Card::Plains])
            .actions(&[
                TO_UPKEEP,
                ME.casts(VORTEX),
                ME.targets_player(THEM),
                wins(ME),
            ]),
    ];
}
