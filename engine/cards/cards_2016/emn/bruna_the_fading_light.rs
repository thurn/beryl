// Generated from the card store by `just card header`: notes go below the end marker.
//! Bruna, the Fading Light {5}{W}{W}
//! Legendary Creature — Angel Horror, 5/7
//!
//! When you cast this spell, you may return target Angel or Human creature card from your graveyard
//! to the battlefield.
//! Flying, vigilance
//! (Melds with Gisela, the Broken Blade.)
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = card_with_shape(
    Card::BrunaTheFadingLight,
    PrintedShape::BrunaTheFadingLight,
    &[Printed::BrunaTheFadingLight
        .keywords(Keywords::FLYING.union(Keywords::VIGILANCE))
        .abilities(&[Ability::triggered(&Trigger::CAST)
            .on_the_stack()
            .target(&TargetSpec::graveyard_card(Filter::All(&[
                Filter::Types(Types::CREATURE),
                Filter::AnyOf(&[
                    Filter::Subtype(Subtype::Angel),
                    Filter::Subtype(Subtype::Human),
                ]),
                Filter::Whose(&Players::You),
            ])))
            .effect(&may(you(), &return_to_battlefield(target())))])],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const BRUNA: &str = Card::BrunaTheFadingLight;
    const GISELA: &str = Card::GiselaTheBrokenBlade;
    const MOTHER: &str = Card::MotherOfRunes;

    /// P0 can cast Bruna and then a Counterspell, with Gisela (an Angel) and Mother of Runes
    /// (a Human) in P0's graveyard.
    const CAST: Scenario = Scenario::setup()
        .battlefield(
            P0,
            &[
                Card::Plains,
                Card::Plains,
                Card::Plains,
                Card::Plains,
                Card::Plains,
                Card::Plains,
                Card::Plains,
                Card::Island,
                Card::Island,
            ],
        )
        .hand(P0, &[BRUNA, Prop::Instant])
        .graveyard(P0, &[GISELA, MOTHER]);

    pub const SCENARIOS: &[Scenario] = &[
        // The ability triggers as Bruna is cast, and its controller chooses to return Gisela
        // while Bruna is still on the stack. The Counterspell cast after that is not Bruna,
        // so Mother of Runes stays in the graveyard, and it counters Bruna.
        CAST.titled("Casting Bruna returns an Angel from your graveyard")
            .interaction(&[GISELA, MOTHER])
            .actions(&[
                ME.casts(BRUNA),
                ME.targets(GISELA),
                Resolve,
                ME.yes(),
                ME.controls(GISELA, 1),
                ME.casts(Prop::Instant),
                Resolve,
                ME.has(Zone::Graveyard, BRUNA, 1),
                ME.has(Zone::Graveyard, MOTHER, 1),
            ]),
        // "You may": its controller can leave the card where it is.
        CAST.titled("Bruna's controller may leave the card in the graveyard")
            .interaction(&[GISELA, MOTHER])
            .actions(&[
                ME.casts(BRUNA),
                ME.targets(GISELA),
                Resolve,
                ME.no(),
                ME.has(Zone::Graveyard, GISELA, 1),
                Resolve,
                ME.controls(BRUNA, 1),
            ]),
    ];
}
