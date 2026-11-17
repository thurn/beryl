// Generated from the card store by `just card header`: notes go below the end marker.
//! Aura Finesse {U}
//! Instant
//!
//! Attach target Aura you control to target creature.
//! Draw a card.
// End of the generated card header.

use crate::prelude::*;

/// Moves an Aura and draws a card. The Aura's enchant ability still limits where it can attach.
pub static CARD: CardDef = Printed::AuraFinesse
    .abilities(&[Ability::spell()
        .targeting(&[
            TargetSpec::permanent(Filter::All(&[
                Filter::Subtype(Subtype::Aura),
                Filter::Whose(&Players::You),
            ])),
            TargetSpec::creature(),
        ])
        .effect(&sequence(&[
            attach(target(), nth_target(1)),
            draw(you(), 1),
        ]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] =
        &[
            Scenario::new("Aura Finesse moves Holy Strength and draws a card")
                .interaction(&[Card::HolyStrength])
                .hand(P0, &[Card::AuraFinesse, Card::HolyStrength])
                .battlefield(
                    P0,
                    &[Card::Island, Card::Plains, Prop::Vanilla, Prop::Flier],
                )
                .library(P0, &[Card::Island])
                .actions(&[
                    ME.casts(Card::HolyStrength),
                    ME.targets(Prop::Vanilla),
                    Resolve,
                    ME.pt(Prop::Vanilla, 3, 3),
                    ME.casts(Card::AuraFinesse),
                    ME.targets(Prop::Flier),
                    Resolve,
                    ME.pt(Prop::Vanilla, 2, 1),
                    ME.pt(Prop::Flier, 4, 3),
                    ME.has(Zone::Hand, Card::Island, 1),
                ]),
        ];
}
