// Generated from the card store by `just card header`: notes go below the end marker.
//! Spell Snare {U}
//! Instant
//!
//! Counter target spell with mana value 2.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::SpellSnare
    .abilities(&[Ability::spell()
        .target(&TargetSpec::one(
            TargetKind::Spell,
            Filter::All(&[
                Filter::ManaValueAtMost(2),
                Filter::Not(&Filter::ManaValueAtMost(1)),
            ]),
        ))
        .effect(&counter(target()))])
    .card();
