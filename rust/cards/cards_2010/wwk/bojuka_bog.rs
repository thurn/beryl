// Generated from the card store by `just card header`: notes go below the end marker.
//! Bojuka Bog
//! Land
//!
//! This land enters tapped.
//! When this land enters, exile target player's graveyard.
//! {T}: Add {B}.
// End of the generated card header.

use crate::prelude::*;

/// Implemented for scenarios where a land loses its enters ability. The Bog enters tapped,
/// can exile either player's graveyard when it enters, and taps for {B} once untapped.
pub static CARD: CardDef = Printed::BojukaBog
    .abilities(&[
        Ability::replacing(&Replacement::ENTERS_TAPPED),
        Ability::triggered(&Trigger::ENTERS)
            .target(&TargetSpec::player())
            .effect(&exile(graveyard_of(&target_player()))),
        Ability::tap_for_mana(ManaSet::one(ManaColor::Black)),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const BOG: &str = Card::BojukaBog;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("the Bog enters tapped and exiles the chosen graveyard")
            .hand(P0, &[BOG])
            .graveyard(P0, &[Prop::Artifact])
            .graveyard(P1, &[Prop::Flier, Prop::Instant])
            .actions(&[
                ME.plays(BOG),
                ME.targets_player(THEM),
                ME.tapped(BOG, 1),
                Resolve,
                THEM.has(Zone::Exile, Prop::Flier, 1),
                THEM.has(Zone::Exile, Prop::Instant, 1),
                ME.has(Zone::Graveyard, Prop::Artifact, 1),
            ]),
        Scenario::new("an untapped Bog pays for a black spell")
            .interaction(&[Card::Damn])
            .battlefield(P0, &[BOG, Card::Swamp, Prop::Flier])
            .battlefield(P1, &[Prop::Vanilla])
            .hand(P0, &[Card::Damn])
            .actions(&[
                ME.casts(Card::Damn),
                ME.targets(Prop::Vanilla),
                Resolve,
                ME.tapped(BOG, 1),
                THEM.has(Zone::Graveyard, Prop::Vanilla, 1),
            ]),
    ];
}
