// Generated from the card store by `just card header`: notes go below the end marker.
//! Plains
//! Basic Land — Plains
//!
//! ({T}: Add {W}.)
// End of the generated card header.

use crate::prelude::*;

/// [CR 305.6](https://yawgatog.com/resources/magic-rules/#R3056): "An object with the land
/// card type and a basic land type has the intrinsic ability '{T}: Add [mana symbol],' ... For
/// Plains, [mana symbol] is {W}".
pub static CARD: CardDef = Printed::Plains
    .abilities(&[Ability::tap_for_mana(ManaSet::one(ManaColor::White))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    /// [CR 305.2](https://yawgatog.com/resources/magic-rules/#R3052): "A player can normally play
    /// one land during their turn". Two Plains in hand are one candidate (§7.2).
    pub const SCENARIOS: &[Scenario] = &[Scenario::new("one Plains a turn, untapped")
        .hand(P0, &[Card::Plains, Card::Plains])
        .actions(&[
            ME.deciding(DecisionKind::Priority, 2),
            ME.plays(Card::Plains),
            advance_to(1, TurnStep::PostcombatMain),
            ME.deciding(DecisionKind::Priority, 1),
            ME.tapped(Card::Plains, 0),
            advance_to(3, TurnStep::PrecombatMain),
            // The other Plains and the Island drawn this turn.
            ME.deciding(DecisionKind::Priority, 3),
            ME.plays(Card::Plains),
            advance_to(3, TurnStep::PostcombatMain),
            ME.has(Zone::Battlefield, Card::Plains, 2),
            ME.has(Zone::Hand, Card::Island, 1),
        ])];
}
