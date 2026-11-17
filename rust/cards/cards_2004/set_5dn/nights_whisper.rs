// Generated from the card store by `just card header`: notes go below the end marker.
//! Night's Whisper {1}{B}
//! Sorcery
//!
//! You draw two cards and lose 2 life.
// End of the generated card header.

use crate::prelude::*;

/// The caster draws two cards and loses two life.
pub static CARD: CardDef = Printed::NightsWhisper
    .abilities(&[Ability::spell().effect(&sequence(&[draw(you(), 2), lose_life(you(), 2)]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] =
        &[
            Scenario::new("Night's Whisper draws two cards and loses two life")
                .hand(P0, &[Card::NightsWhisper])
                .battlefield(P0, &[Card::Swamp; 2])
                .library(P0, &[Card::Island, Card::Plains])
                .actions(&[
                    ME.casts(Card::NightsWhisper),
                    Resolve,
                    ME.has(Zone::Hand, Card::Island, 1),
                    ME.has(Zone::Hand, Card::Plains, 1),
                    ME.life(18),
                ]),
        ];
}
