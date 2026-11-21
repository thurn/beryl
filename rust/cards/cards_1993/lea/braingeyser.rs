// Generated from the card store by `just card header`: notes go below the end marker.
//! Braingeyser {X}{U}{U}
//! Sorcery
//!
//! Target player draws X cards.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::Braingeyser
    .abilities(&[Ability::spell()
        .target(&TargetSpec::player())
        .effect(&Effect::Draw {
            player: target_player(),
            count: x(),
        })])
    .card();
