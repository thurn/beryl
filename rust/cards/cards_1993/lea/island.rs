// Generated from the card store by `just card header`: notes go below the end marker.
//! Island
//! Basic Land — Island
//!
//! ({T}: Add {U}.)
// End of the generated card header.

use crate::prelude::*;

/// [CR 305.6]: "An object with the land card type and a basic land type has the intrinsic ability
/// '{T}: Add [mana symbol],' ... for Islands, {U}".
///
/// [CR 305.6]: https://yawgatog.com/resources/magic-rules/#R3056
pub static CARD: CardDef = Printed::Island
    .abilities(&[Ability::tap_for_mana(ManaSet::one(ManaColor::Blue))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    /// [CR 305.1]: "A player who has priority may play a land card from their hand during a main
    /// phase of their turn when the stack is empty." Not in another player's turn: when the active
    /// player passes, the other player with an Island in hand has nothing to decide.
    ///
    /// [CR 305.1]: https://yawgatog.com/resources/magic-rules/#R3051
    pub const SCENARIOS: &[Scenario] = &[Scenario::new("an Island only in its owner's main phase")
        .at(2, P1, TurnStep::PrecombatMain)
        .hand(P0, &[Card::Island])
        .hand(P1, &[Card::Island])
        .actions(&[
            THEM.answers(DecisionKind::Priority, Pick::Pass),
            // Then straight to the active player's next main phase.
            THEM.deciding(DecisionKind::Priority, 2),
            THEM.plays(Card::Island),
            advance_to(2, TurnStep::End),
            THEM.has(Zone::Battlefield, Card::Island, 1),
            ME.has(Zone::Hand, Card::Island, 1),
        ])];
}
