// Generated from the card store by `just card header`: notes go below the end marker.
//! Urborg Emissary {2}{B}
//! Creature — Human Wizard, 3/1
//!
//! Kicker {1}{U} (You may pay an additional {1}{U} as you cast this spell.)
//! When this creature enters, if it was kicked, return target permanent to its owner's hand.
// End of the generated card header.

use crate::prelude::*;

/// Implemented to test Cavern of Souls paying the blue part of a black spell's kicker.
/// The enter trigger returns a permanent only if the caster paid kicker.
pub static CARD: CardDef = Printed::UrborgEmissary
    .abilities(&[Ability::triggered(&Trigger::ENTERS)
        .only_if(&Condition::Paid(CastingKind::Kicker))
        .target(&TargetSpec::permanent(Filter::Any))
        .effect(&return_to_hand(target()))])
    .casting(&[CastingOption::Kicker {
        cost: ManaCost::new(1, [0, 1, 0, 0, 0, 0]),
        min_x: 0,
    }])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const EMISSARY: &str = Card::UrborgEmissary;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Urborg Emissary kicked returns an opposing permanent")
            .battlefield(
                P0,
                &[
                    Card::Swamp,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .battlefield(P1, &[Prop::Flier])
            .hand(P0, &[EMISSARY, Prop::Vanilla])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: EMISSARY,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                ME.tapped(Card::Island, 4),
                Resolve,
                ME.targets(Prop::Flier),
                Resolve,
                THEM.has(Zone::Hand, Prop::Flier, 1),
                // The kicked Emissary does not trigger again for a later creature.
                ME.casts(Prop::Vanilla),
                Resolve,
                stack(0),
                ME.controls(Prop::Vanilla, 1),
            ]),
        Scenario::new("Urborg Emissary unkicked leaves the opposing permanent")
            .battlefield(P0, &[Card::Swamp; 3])
            .battlefield(P1, &[Prop::Flier])
            .hand(P0, &[EMISSARY])
            .actions(&[
                ME.casts(EMISSARY),
                Resolve,
                stack(0),
                ME.controls(EMISSARY, 1),
                THEM.controls(Prop::Flier, 1),
            ]),
    ];
}
