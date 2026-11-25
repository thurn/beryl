// Generated from the card store by `just card header`: notes go below the end marker.
//! Deathmark {B}
//! Sorcery
//!
//! Destroy target green or white creature.
// End of the generated card header.

use crate::prelude::*;

/// Implemented for scenarios that change color words.
pub static CARD: CardDef = Printed::Deathmark
    .abilities(&[Ability::spell()
        .target(&TargetSpec::permanent(Filter::All(&[
            creature(),
            Filter::Colors(Colors::GREEN.union(Colors::WHITE)),
        ])))
        .effect(&destroy(target()))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] =
        &[
            Scenario::new("Deathmark can destroy creatures of either named color")
                .interaction(&[Card::WildNacatl])
                .battlefield(P0, &[Card::Swamp; 2])
                .battlefield(P1, &[Prop::Flier, Card::WildNacatl, Prop::Vanilla])
                .hand(P0, &[Card::Deathmark; 2])
                .actions(&[
                    ME.casts(Card::Deathmark),
                    ME.targets(Prop::Flier),
                    Resolve,
                    ME.casts(Card::Deathmark),
                    THEM.has(Zone::Graveyard, Prop::Flier, 1),
                    THEM.has(Zone::Graveyard, Card::WildNacatl, 1),
                    THEM.controls(Prop::Vanilla, 1),
                ]),
        ];
}
