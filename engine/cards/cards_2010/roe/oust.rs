// Generated from the card store by `just card header`: notes go below the end marker.
//! Oust {W}
//! Sorcery
//!
//! Put target creature into its owner's library second from the top. Its controller gains 3 life.
// End of the generated card header.

use crate::prelude::*;

/// With an empty library, the creature becomes its top card. The life goes to the player
/// who controlled the creature on the battlefield, who may differ from its owner
/// ([CR 608.2h]).
///
/// [CR 608.2h]: https://yawgatog.com/resources/magic-rules/#R6082h
pub static CARD: CardDef = Printed::Oust
    .abilities(&[Ability::spell()
        .target(&TargetSpec::creature())
        .effect(&sequence(&[
            put_into_library(target(), LibraryPosition::FromTop(2)),
            gain_life(its_controller(), 3),
        ]))])
    .card();
