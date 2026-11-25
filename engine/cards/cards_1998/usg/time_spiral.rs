// Generated from the card store by `just card header`: notes go below the end marker.
//! Time Spiral {4}{U}{U}
//! Sorcery
//!
//! Exile Time Spiral. Each player shuffles their hand and graveyard into their library, then draws
//! seven cards. You untap up to six lands.
// End of the generated card header.

use crate::prelude::*;

/// Time Spiral moves itself from the stack to exile before shuffling the graveyards.
/// It stays in exile because the final part of resolution moves the spell from the stack
/// to its owner's graveyard ([CR 608.2n]): "As the final part of an instant or sorcery
/// spell’s resolution, the spell is put into its owner’s graveyard."
///
/// [CR 608.2n]: https://yawgatog.com/resources/magic-rules/#R6082n
pub static CARD: CardDef = Printed::TimeSpiral
    .abilities(&[Ability::spell().effect(&sequence(&[
        exile(this()),
        shuffle_into(
            each_player(),
            Objects::CardsIn {
                players: &each_player(),
                zones: &[Zone::Hand, Zone::Graveyard],
            },
        ),
        draw(each_player(), 7),
        untap_up_to(you(), 6, permanents(land())),
    ]))])
    .card();
