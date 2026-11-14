// Generated from the card store by `just card header`: notes go below the end marker.
//! Mountain
//! Basic Land — Mountain
//!
//! ({T}: Add {R}.)
// End of the generated card header.

use crate::prelude::*;

/// [CR 305.6](https://yawgatog.com/resources/magic-rules/#R3056): "An object with the land card type and a basic land type has the intrinsic ability
/// '{T}: Add [mana symbol],' ... for Mountains, {R}".
///
/// [CR 305.6]: https://yawgatog.com/resources/magic-rules/#R3056
pub static CARD: CardDef = Printed::Mountain
    .abilities(&[Ability::tap_for_mana(ManaSet::one(ManaColor::Red))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    /// A Mountain pays for a spell by adding one mana ([CR 305.6]). No implemented spell costs
    /// {R}, so the scenario pays Leonin Scimitar's {1}, which any mana can pay.
    ///
    /// [CR 305.6]: https://yawgatog.com/resources/magic-rules/#R3056
    pub const SCENARIOS: &[Scenario] = &[Scenario::new("a Mountain pays a generic cost")
        .battlefield(P0, &[Card::Mountain])
        .hand(P0, &[Prop::Equipment])
        .actions(&[
            ME.casts(Prop::Equipment),
            Resolve,
            ME.tapped(Card::Mountain, 1),
            ME.has(Zone::Battlefield, Prop::Equipment, 1),
            ME.mana(ManaColor::Red, 0),
        ])];
}
