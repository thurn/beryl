// Generated from the card store by `just card header`: notes go below the end marker.
//! Spellseeker {2}{U}
//! Creature — Human Wizard, 1/1
//!
//! When this creature enters, you may search your library for an instant or sorcery card with mana
//! value 2 or less, reveal it, put it into your hand, then shuffle.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::Spellseeker
    .abilities(&[when_enters(&may(
        you(),
        &search(
            you(),
            Filter::All(&[instant_or_sorcery(), Filter::ManaValueAtMost(2)]),
            Found::RevealToHand,
        ),
    ))])
    .card();
