// Generated from the card store by `just card header`: notes go below the end marker.
//! Floodfarm Verge
//! Land
//!
//! {T}: Add {W}.
//! {T}: Add {U}. Activate only if you control a Plains or an Island.
// End of the generated card header.

use crate::prelude::*;

/// The {U} ability's restriction reads the land's subtypes, so a Tundra, a Plains Island, counts,
/// as a basic does ([CR 305.6]): "The basic land types are Plains, Island, Swamp, Mountain, and
/// Forest."
///
/// [CR 305.6]: https://yawgatog.com/resources/magic-rules/#R3056
pub static CARD: CardDef = Printed::FloodfarmVerge
    .abilities(&[
        Ability::tap_for_mana(ManaSet::one(ManaColor::White)),
        Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)).if_you_control(&Filter::AnyOf(&[
            Filter::Subtype(Subtype::Plains),
            Filter::Subtype(Subtype::Island),
        ])),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const VERGE: &str = Card::FloodfarmVerge;

    pub const SCENARIOS: &[Scenario] = &[
        // Tundra is a Plains and an Island: its {U} and the Verge's pay for Counterspell.
        Scenario::new("Floodfarm Verge's {U} with a Tundra")
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[VERGE, Card::Tundra])
            .hand(P0, &[Card::Counterspell])
            // P1's Plains for the artifact.
            .battlefield(P1, &[Card::Plains])
            .hand(P1, &[Prop::Artifact, Card::Plains])
            .actions(&[
                // P1 casts the artifact in their main phase, with a land still to play.
                THEM.casts(Prop::Artifact),
                ME.casts(Card::Counterspell),
                ME.tapped(VERGE, 1),
                THEM.has(Zone::Graveyard, Prop::Artifact, 1),
            ]),
        // A second Verge is neither a Plains nor an Island, so each makes only {W}: P0 can't
        // cast Counterspell and has nothing to do, and the artifact resolves.
        Scenario::new("Floodfarm Verge's {U} without a Plains or an Island")
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[VERGE, VERGE])
            .hand(P0, &[Card::Counterspell])
            // P1's Plains for the artifact.
            .battlefield(P1, &[Card::Plains])
            .hand(P1, &[Prop::Artifact, Card::Plains])
            .actions(&[
                // P1 casts the artifact in their main phase, with a land still to play.
                THEM.casts(Prop::Artifact),
                Resolve,
                THEM.controls(Prop::Artifact, 1),
                ME.tapped(VERGE, 0),
            ]),
        // A basic Plains enables the {U}: with the Plains's {W} for the generic mana, the
        // two-drop creature.
        Scenario::new("Floodfarm Verge's {U} with a Plains")
            .battlefield(P0, &[VERGE, Card::Plains])
            .hand(P0, &[Prop::SmallCreature])
            .actions(&[
                ME.casts(Prop::SmallCreature),
                Resolve,
                ME.controls(Prop::SmallCreature, 1),
            ]),
        // Alone, its {W} pays for the artifact. The land still to play keeps the game in P0's
        // main phase.
        Scenario::new("Floodfarm Verge's {W}")
            .battlefield(P0, &[VERGE])
            .hand(P0, &[Prop::Artifact, Card::Plains])
            .actions(&[
                ME.casts(Prop::Artifact),
                ME.tapped(VERGE, 1),
                Resolve,
                ME.controls(Prop::Artifact, 1),
            ]),
    ];
}
