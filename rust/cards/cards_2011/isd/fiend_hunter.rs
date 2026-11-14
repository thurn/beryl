// Generated from the card store by `just card header`: notes go below the end marker.
//! Fiend Hunter {1}{W}{W}
//! Creature — Human Cleric, 1/3
//!
//! When this creature enters, you may exile another target creature.
//! When this creature leaves the battlefield, return the exiled card to the battlefield under its
//! owner's control.
// End of the generated card header.

use crate::prelude::*;

/// The enters ability exiles the creature, linked to this Fiend Hunter
/// ([CR 607.2a]): "If an object has an activated or triggered ability printed on it that
/// instructs a player to exile one or more cards and an ability printed on it that refers either
/// to "the exiled cards" or to cards "exiled with [this object]," these abilities are linked."
/// If Fiend Hunter has already left the battlefield when the enters ability resolves, nothing is
/// linked and the creature stays exiled for good: this is not an "until" effect ([CR 610.3]).
///
/// The leaves ability looks back in time ([CR 603.10a]) and returns the cards exiled with the
/// Hunter, wherever the Hunter went, under their owners' control ([CR 110.2a]). A token that was
/// exiled has ceased to exist ([CR 111.7]) and does not return; any other card comes back as a
/// new object ([CR 400.7]).
///
/// [CR 607.2a]: https://yawgatog.com/resources/magic-rules/#R6072a
/// [CR 610.3]: https://yawgatog.com/resources/magic-rules/#R6103
/// [CR 603.10a]: https://yawgatog.com/resources/magic-rules/#R60310a
/// [CR 110.2a]: https://yawgatog.com/resources/magic-rules/#R1102a
/// [CR 111.7]: https://yawgatog.com/resources/magic-rules/#R1117
/// [CR 400.7]: https://yawgatog.com/resources/magic-rules/#R4007
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
            .battlefield(P1, &[Prop::Creature])
            .library(P0, &[Card::Plains])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.casts(HUNTER),
                Resolve,
                Resolve,
                ME.yes(),
                THEM.has(Zone::Exile, Prop::Creature, 1),
                THEM.controls(Prop::Creature, 0),
                // Fiend Hunter is the only creature left to target, so it is chosen silently.
                ME.casts(Card::GetLost),
                Resolve,
                Resolve,
                ME.has(Zone::Graveyard, HUNTER, 1),
                THEM.has(Zone::Exile, Prop::Creature, 0),
                THEM.controls(Prop::Creature, 1),
            ]),
        // The leaves ability triggers for every zone, so bouncing Fiend Hunter with Jace returns the
        // creature too.
        Scenario::new("Fiend Hunter returned to hand returns the exiled creature")
            .hand(P0, &[HUNTER])
            .battlefield(
                P0,
                &[Card::Plains, Card::Plains, Card::Plains, Prop::Planeswalker],
            )
            .battlefield(P1, &[Prop::SmallCreature])
            .library(P0, &[Card::Plains])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.casts(HUNTER),
                Resolve,
                Resolve,
                ME.yes(),
                THEM.has(Zone::Exile, Prop::SmallCreature, 1),
                // Fiend Hunter is the only creature on the battlefield, so Jace targets it
                // silently.
                ME.activates(Prop::Planeswalker, 2), // −1
                Resolve,
                Resolve,
                ME.has(Zone::Hand, HUNTER, 1),
                THEM.has(Zone::Exile, Prop::SmallCreature, 0),
                THEM.controls(Prop::SmallCreature, 1),
            ]),
        // If Fiend Hunter leaves with its enters ability still on the stack, the leaves ability
        // resolves first and returns nothing; the enters ability then exiles the creature for
        // good.
        Scenario::new("Fiend Hunter leaving before its enters ability resolves exiles for good")
            .interaction(&[Card::GetLost])
            .hand(P0, &[HUNTER, Card::GetLost])
            .battlefield(P0, &[Card::Plains; 5])
            .battlefield(P1, &[Prop::Creature])
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
                THEM.has(Zone::Exile, Prop::Creature, 1),
                THEM.controls(Prop::Creature, 0),
            ]),
    ];
}
