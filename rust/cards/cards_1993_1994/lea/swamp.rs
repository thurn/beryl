// Generated from the card store by `just card header`: notes go below the end marker.
//! Swamp
//! Basic Land — Swamp
//!
//! ({T}: Add {B}.)
// End of the generated card header.

use crate::{names, printed};
use beryl_card::{Ability, CardDef, ManaColor, ManaSet, card};

/// [CR 305.6](https://yawgatog.com/resources/magic-rules/#R3056): "An object with the land
/// card type and a basic land type has the intrinsic ability '{T}: Add [mana symbol],' ... for
/// Swamps, {B}".
pub static CARD: CardDef = card(
    names::SWAMP,
    &[printed::SWAMP.abilities(&[Ability::tap_for_mana(ManaSet::one(ManaColor::Black))])],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_card::Zone;
    use beryl_scenario_types::{Action, Assertion, P0, P1, Permanent, Scenario};

    use crate::names;

    /// Two Swamps pay Damn's {B}{B}, which two Plains could not: Damn is the one card that asks
    /// for black mana.
    pub const SCENARIOS: &[Scenario] = &[Scenario::new("two Swamps pay {B}{B}")
        .interaction(&[names::DAMN])
        .battlefield(
            P0,
            &[Permanent::new(names::SWAMP), Permanent::new(names::SWAMP)],
        )
        .hand(P0, &[names::DAMN])
        .battlefield(P1, &[Permanent::new(names::ELITE_SPELLBINDER)])
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::cast(P0, names::DAMN),
            Action::Resolve,
            Action::assert(Assertion::Tapped {
                player: P0,
                card: names::SWAMP,
                count: 2,
            }),
            Action::assert(Assertion::Count {
                player: P1,
                zone: Zone::Graveyard,
                card: Some(names::ELITE_SPELLBINDER),
                count: 1,
            }),
        ])];
}
