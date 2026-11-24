// Generated from the card store by `just card header`: notes go below the end marker.
//! Jace, the Mind Sculptor {2}{U}{U}
//! Legendary Planeswalker — Jace, loyalty 3
//!
//! +2: Look at the top card of target player's library. You may put that card on the bottom of that
//! player's library.
//! 0: Draw three cards, then put two cards from your hand on top of your library in any order.
//! −1: Return target creature to its owner's hand.
//! −12: Exile all cards from target player's library, then that player shuffles their hand into
//! their library.
// End of the generated card header.

use crate::prelude::*;

/// The 0 ability puts each selected card on top of the previous one, so the second card
/// chosen is the first card drawn.
pub static CARD: CardDef = Printed::JaceTheMindSculptor
    .abilities(&[
        Ability::loyalty(2)
            .target(&TargetSpec::player())
            .effect(&fateseal(you(), target_player())),
        Ability::loyalty(0).effect(&sequence(&[draw(you(), 3), put_back(you(), 2)])),
        Ability::loyalty(-1)
            .target(&TargetSpec::creature())
            .effect(&return_to_hand(target())),
        Ability::loyalty(-12)
            .target(&TargetSpec::player())
            .effect(&sequence(&[
                exile(library_of(&target_player())),
                shuffle_into(target_player(), hand_of(&target_player())),
            ])),
    ])
    .card();
