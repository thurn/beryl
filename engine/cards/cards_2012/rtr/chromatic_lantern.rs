// Generated from the card store by `just card header`: notes go below the end marker.
//! Chromatic Lantern {3}
//! Artifact
//!
//! Lands you control have "{T}: Add one mana of any color."
//! {T}: Add one mana of any color.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::ChromaticLantern
    .abilities(&[
        Ability::static_ability(&LayerSpec {
            affected: permanents(Filter::All(&[
                Filter::Types(Types::LAND),
                Filter::Whose(&Players::You),
            ])),
            condition: None,
            modifications: &[Modification::grant(0)],
        }),
        Ability::tap_for_mana(ManaSet::any_color(1)),
    ])
    .grants(&[Ability::tap_for_mana(ManaSet::any_color(1))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Chromatic Lantern taps for white mana to cast a white spell")
            .interaction(&[Card::HolyStrength])
            .battlefield(P0, &[Card::ChromaticLantern, Prop::Vanilla])
            .hand(P0, &[Card::HolyStrength])
            .actions(&[
                ME.casts(Card::HolyStrength),
                Resolve,
                ME.controls(Card::HolyStrength, 1),
            ]),
        Scenario::new("a Forest taps for white mana under Chromatic Lantern")
            .battlefield(P0, &[Card::ChromaticLantern, Card::Forest, Card::Forest])
            .hand(P0, &[Prop::Flier])
            .actions(&[ME.casts(Prop::Flier), Resolve, ME.controls(Prop::Flier, 1)]),
    ];
}
