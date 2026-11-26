// Generated from the card store by `just card header`: notes go below the end marker.
//! Parallel Lives {3}{G}
//! Enchantment
//!
//! If an effect would create one or more tokens under your control, it creates twice that many of
//! those tokens instead.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::ParallelLives
    .abilities(&[Ability::replacing(&Replacement::tokens_created(
        Players::You,
        Rewrite::Multiply(2),
    ))])
    .card();
