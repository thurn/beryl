// Generated from the card store by `just card header`: notes go below the end marker.
//! Timetwister {2}{U}
//! Sorcery
//!
//! Each player shuffles their hand and graveyard into their library, then draws seven cards. (Then
//! put Timetwister into its owner's graveyard.)
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::Timetwister
    .abilities(&[Ability::spell().effect(&sequence(&[
        shuffle_into(
            each_player(),
            Objects::CardsIn {
                players: &each_player(),
                zones: &[Zone::Hand, Zone::Graveyard],
            },
        ),
        draw(each_player(), 7),
    ]))])
    .card();
