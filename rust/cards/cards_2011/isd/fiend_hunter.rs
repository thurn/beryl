// Generated from the card store by `just card header`: notes go below the end marker.
//! Fiend Hunter {1}{W}{W}
//! Creature — Human Cleric, 1/3
//!
//! When this creature enters, you may exile another target creature.
//! When this creature leaves the battlefield, return the exiled card to the battlefield under its
//! owner's control.
// End of the generated card header.

use crate::prelude::*;

/// If Fiend Hunter leaves before its enters ability resolves, its leaves ability returns
/// nothing and the chosen creature stays exiled ([CR 607.2a]). The linked abilities resolve
/// independently.
///
/// [CR 607.2a]: https://yawgatog.com/resources/magic-rules/#R6072a
pub static CARD: CardDef = Printed::FiendHunter
    .abilities(&[
        Ability::triggered(&Trigger::ENTERS)
            .target(&TargetSpec::creature().other())
            .effect(&may(you(), &exile_linked(target()))),
        Ability::triggered(&Trigger::LEAVES).effect(&return_to_battlefield(exiled_with_this())),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const HUNTER: &str = Card::FiendHunter;

    pub const SCENARIOS: &[Scenario] = &[
        // Fiend Hunter exiles P1's creature, then dies to Get Lost; the creature comes back under
        // its owner's control.
        Scenario::new("Fiend Hunter dying returns the exiled creature")
            .interaction(&[Card::GetLost])
            .hand(P0, &[HUNTER, Card::GetLost])
            .battlefield(P0, &[Card::Plains; 5])
            .battlefield(P1, &[Prop::Flier])
            .library(P0, &[Card::Plains])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.casts(HUNTER),
                Resolve,
                Resolve,
                ME.yes(),
                THEM.has(Zone::Exile, Prop::Flier, 1),
                THEM.controls(Prop::Flier, 0),
                // Fiend Hunter is the only creature left to target, so it is chosen silently.
                ME.casts(Card::GetLost),
                Resolve,
                Resolve,
                ME.has(Zone::Graveyard, HUNTER, 1),
                THEM.has(Zone::Exile, Prop::Flier, 0),
                THEM.controls(Prop::Flier, 1),
            ]),
        // If Fiend Hunter leaves with its enters ability still on the stack, the leaves ability
        // resolves first and returns nothing; the enters ability then exiles the creature for
        // good.
        Scenario::new("Fiend Hunter leaving before its enters ability resolves exiles for good")
            .interaction(&[Card::GetLost])
            .hand(P0, &[HUNTER, Card::GetLost])
            .battlefield(P0, &[Card::Plains; 5])
            .battlefield(P1, &[Prop::Flier])
            .library(P0, &[Card::Plains])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.casts(HUNTER),
                Resolve,
                ME.casts(Card::GetLost),
                ME.targets(HUNTER),
                Resolve,
                Resolve,
                Resolve,
                ME.yes(),
                THEM.has(Zone::Exile, Prop::Flier, 1),
                THEM.controls(Prop::Flier, 0),
            ]),
    ];
}
