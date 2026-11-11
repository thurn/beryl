//! Predefined tokens more than one card creates.

use beryl_engine::card::{Ability, CardDef, Cost, Face, ManaSet, Subtype, Subtypes, Types};

/// The Treasure token's name: its subtype plus the word "Token"
/// ([CR 111.4](https://yawgatog.com/resources/magic-rules/#R1114)).
pub const TREASURE_TOKEN: &str = "Treasure Token";

/// The predefined Treasure token
/// ([CR 111.10a](https://yawgatog.com/resources/magic-rules/#R11110a)): "A Treasure token is a
/// colorless Treasure artifact token with "{T}, Sacrifice this token: Add one mana of any
/// color."" Its ability is a mana ability
/// ([CR 605.1a](https://yawgatog.com/resources/magic-rules/#R6051a)): "it doesn't require a
/// target ..., it could add mana to a player's mana pool when it resolves, it's not a loyalty
/// ability ..., and its cost and effect don't move any card to or from a library."
pub static TREASURE: CardDef = CardDef {
    name: TREASURE_TOKEN,
    faces: &[Face {
        types: Types::ARTIFACT,
        subtypes: Subtypes::of(&[Subtype::Treasure]),
        abilities: &[Ability {
            cost: Cost {
                sacrifice_self: true,
                ..Cost::TAP
            },
            // W, U, B, R and G.
            ..Ability::tap_for_mana(ManaSet {
                colors: 0b1_1111,
                amount: 1,
            })
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};
