// Generated from the card store by `just card header`: notes go below the end marker.
//! Young Pyromancer {1}{R}
//! Creature — Human Shaman, 2/1
//!
//! Whenever you cast an instant or sorcery spell, create a 1/1 red Elemental creature token.
// End of the generated card header.

use crate::prelude::*;

pub static ELEMENTAL: CardDef = Token::ElementalToken.card();

pub static CARD: CardDef = Printed::YoungPyromancer
    .abilities(&[
        Ability::triggered(&Trigger::casts(Players::You, instant_or_sorcery()))
            .effect(&create_tokens(you(), &ELEMENTAL, 1)),
    ])
    .card()
    .creating(&[&ELEMENTAL]);
