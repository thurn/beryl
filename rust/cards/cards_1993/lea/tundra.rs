// Generated from the card store by `just card header`: notes go below the end marker.
//! Tundra
//! Land — Plains Island
//!
//! ({T}: Add {W} or {U}.)
// End of the generated card header.

use crate::prelude::*;

/// Not basic, but it has both basic land types, and with them both intrinsic mana abilities
/// ([CR 305.6]): "An object with the land card type and a basic land type has the intrinsic ability
/// '{T}: Add [mana symbol],' even if the text box doesn't actually contain that text". They are
/// mana abilities ([CR 605.1a]): an activated ability that "doesn't require a target ..., it could
/// add mana to a player's mana pool when it resolves, it's not a loyalty ability ..., and its cost
/// and effect don't move any card to or from a library."
///
/// [CR 305.6]: https://yawgatog.com/resources/magic-rules/#R3056
/// [CR 605.1a]: https://yawgatog.com/resources/magic-rules/#R6051a
pub static CARD: CardDef = Printed::Tundra
    .abilities(&[
        Ability::tap_for_mana(ManaSet::one(ManaColor::White)),
        Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    /// Tundra is not a Plains or an Island card, so it is a candidate of its own beside them.
    pub const SCENARIOS: &[Scenario] = &[Scenario::new("Tundra played beside a Plains in hand")
        .hand(P0, &[Card::Tundra, Card::Plains])
        .actions(&[
            ME.deciding(DecisionKind::Priority, 3),
            ME.plays(Card::Tundra),
            advance_to(1, TurnStep::PostcombatMain),
            ME.has(Zone::Battlefield, Card::Tundra, 1),
            ME.tapped(Card::Tundra, 0),
            ME.has(Zone::Hand, Card::Plains, 1),
        ])];
}
