// Generated from the card store by `just card header`: notes go below the end marker.
//! Sylvan Scrying {1}{G}
//! Sorcery
//!
//! Search your library for a land card, reveal it, put it into your hand, then shuffle.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::SylvanScrying
    .abilities(&[Ability::spell().effect(&search(you(), land(), Found::RevealToHand))])
    .card();
