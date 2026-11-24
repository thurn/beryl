// Generated from the card store by `just card header`: notes go below the end marker.
//! Cerebral Vortex {1}{U}{R}
//! Instant
//!
//! Target player draws two cards, then Cerebral Vortex deals damage to that player equal to the
//! number of cards they've drawn this turn.
// End of the generated card header.

use crate::prelude::*;

/// The total includes successful draws from this spell. A draw from an empty library adds
/// nothing to the total ([CR 121.1]): "A player draws a card by putting the top card of
/// their library into their hand."
///
/// [CR 121.1]: https://yawgatog.com/resources/magic-rules/#R1211
pub static CARD: CardDef = Printed::CerebralVortex
    .abilities(&[Ability::spell()
        .target(&TargetSpec::player())
        .effect(&sequence(&[
            draw(target_player(), 2),
            Effect::DealDamage {
                to: Recipients::Players(target_player()),
                amount: Amount::CardsDrawn(target_player()),
            },
        ]))])
    .card();
