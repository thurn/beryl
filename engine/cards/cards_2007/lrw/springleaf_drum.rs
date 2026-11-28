// Generated from the card store by `just card header`: notes go below the end marker.
//! Springleaf Drum {1}
//! Artifact
//!
//! {T}, Tap an untapped creature you control: Add one mana of any color.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::SpringleafDrum
    .abilities(&[Ability::mana(
        Cost::of_parts(&[
            CostPart::TAP,
            CostPart::Chosen(
                CostAction::Tap,
                beryl_card::Chosen {
                    zone: Zone::Battlefield,
                    filter: Filter::All(&[
                        Filter::Not(&Filter::Tapped),
                        Filter::Types(Types::CREATURE),
                        Filter::Whose(&Players::You),
                    ]),
                    other: false,
                    measure: beryl_card::Measure::Count(1),
                },
            ),
        ]),
        ManaSet::any_color(1),
    )])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] =
        &[
            Scenario::new("Springleaf Drum taps itself and a creature to add mana")
                .battlefield(P0, &[Card::SpringleafDrum, Prop::ColorlessCreature])
                .hand(P0, &[Prop::Artifact])
                .actions(&[
                    ME.activates(Card::SpringleafDrum, 0),
                    ME.chooses_color(Colors::WHITE),
                    ME.tapped(Card::SpringleafDrum, 1),
                    ME.tapped(Prop::ColorlessCreature, 1),
                    ME.mana(ManaColor::White, 1),
                    ME.casts(Prop::Artifact),
                    Resolve,
                    ME.controls(Prop::Artifact, 1),
                ]),
        ];
}
