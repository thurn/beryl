//! Tundra. Land — Plains Island. "({T}: Add {W} or {U}.)"

use beryl_engine::card::{Ability, CardDef, Face, ManaColor, ManaSet, Subtype, Subtypes, Types};

use crate::names;

/// Not basic, but it has both basic land types, and with them both intrinsic mana abilities
/// ([CR 305.6](https://yawgatog.com/resources/magic-rules/#R3056)): "An object with the land
/// card type and a basic land type has the intrinsic ability '{T}: Add [mana symbol],' even if
/// the text box doesn't actually contain that text". They are mana abilities
/// ([CR 605.1a](https://yawgatog.com/resources/magic-rules/#R6051a)): an activated ability
/// that "doesn't require a target ..., it could add mana to a player's mana pool when it
/// resolves, it's not a loyalty ability ..., and its cost and effect don't move any card to or
/// from a library."
pub static CARD: CardDef = CardDef {
    name: names::TUNDRA,
    faces: &[Face {
        types: Types::LAND,
        subtypes: Subtypes::of(&[Subtype::Plains, Subtype::Island]),
        abilities: &[
            Ability::tap_for_mana(ManaSet::one(ManaColor::White)),
            Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)),
        ],
        ..Face::EMPTY
    }],
};
