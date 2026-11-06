//! Swamp. Basic Land — Swamp.

use beryl_engine::card::{
    Ability, CardDef, Face, ManaColor, ManaSet, Subtype, Subtypes, Supertypes, Types,
};

use beryl_engine::Zone;
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

use crate::names;

/// [CR 305.6](https://yawgatog.com/resources/magic-rules/#R3056): "An object with the land
/// card type and a basic land type has the intrinsic ability '{T}: Add [mana symbol],' ... for
/// Swamps, {B}".
pub static CARD: CardDef = CardDef {
    name: names::SWAMP,
    faces: &[Face {
        supertypes: Supertypes::BASIC,
        types: Types::LAND,
        subtypes: Subtypes::of(&[Subtype::Swamp]),
        abilities: &[Ability::tap_for_mana(ManaSet::one(ManaColor::Black))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// Two Swamps pay Damn's {B}{B}, which two Plains could not.
pub const SCENARIOS: &[Scenario] = &[Scenario::new("two Swamps pay {B}{B}")
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
