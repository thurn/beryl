//! Predefined tokens more than one card creates, and the names of the tokens that another
//! card's scenarios or the rules library's name. Such a token's definition stays in the file of
//! the card that creates it.

use crate::Token;
use beryl_card::{Ability, CardDef, Cost, CostPart, ManaSet, card};

/// A token's name where the effect creating it gives none
/// ([CR 111.4](https://yawgatog.com/resources/magic-rules/#R1114)): "If the spell or ability
/// doesn't specify the name of the token, its name is the same as its subtype(s) plus the word
/// 'Token.'"
pub const HUMAN_TOKEN: &str = "Human Token";
pub const INSECT_TOKEN: &str = "Insect Token";
pub const MAP_TOKEN: &str = "Map Token";
pub const ROGUE_TOKEN: &str = "Rogue Token";
pub const SAMURAI_TOKEN: &str = "Samurai Token";

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
pub static TREASURE: CardDef = card(
    TREASURE_TOKEN,
    &[Token::TreasureToken.abilities(&[Ability::mana(
        Cost::of_parts(&[CostPart::TAP, CostPart::SACRIFICE]),
        // W, U, B, R and G.
        ManaSet {
            colors: 0b1_1111,
            amount: 1,
        },
    )])],
);
