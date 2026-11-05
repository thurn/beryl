//! Island. Basic Land — Island.

use beryl_engine::card::{
    Ability, CardDef, Face, ManaColor, ManaSet, Subtype, Subtypes, Supertypes, Types,
};

use crate::names;

/// [CR 305.6](https://yawgatog.com/resources/magic-rules/#R3056): "An object with the land
/// card type and a basic land type has the intrinsic ability '{T}: Add [mana symbol],' ... for
/// Islands, {U}".
pub static CARD: CardDef = CardDef {
    name: names::ISLAND,
    faces: &[Face {
        supertypes: Supertypes::BASIC,
        types: Types::LAND,
        subtypes: Subtypes::of(&[Subtype::Island]),
        abilities: &[Ability::tap_for_mana(ManaSet::one(ManaColor::Blue))],
        ..Face::EMPTY
    }],
};
