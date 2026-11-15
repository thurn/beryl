// Generated from the card store by `just card header`: notes go below the end marker.
//! Brain Freeze {1}{U}
//! Instant
//!
//! Target player mills three cards.
//! Storm (When you cast this spell, copy it for each spell cast before it this turn. You may choose
//! new targets for the copies.)
// End of the generated card header.

use crate::prelude::*;

/// "Target player mills three cards." Storm copies the spell once for each other spell cast
/// before it this turn ([CR 702.40a]), and each copy may have a new target.
///
/// [CR 702.40a]: https://yawgatog.com/resources/magic-rules/#R70240a
pub static CARD: CardDef = Printed::BrainFreeze
    .abilities(&[
        Ability::spell()
            .target(&TargetSpec::player())
            .effect(&mill(target_player(), 3)),
        Ability::storm(),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const FREEZE: &str = Card::BrainFreeze;
    const SCIMITAR: &str = Prop::Equipment;
    const ISLAND: &str = Card::Island;

    pub const SCENARIOS: &[Scenario] = &[
        // One spell before it: the spell and its copy mill three each.
        Scenario::new("Brain Freeze with storm 1")
            .battlefield(P0, &[Card::Island, Card::Island, Card::Plains])
            .hand(P0, &[SCIMITAR, FREEZE])
            .library(P1, &[Card::Plains; 10])
            .actions(&[
                ME.casts(SCIMITAR),
                Resolve,
                ME.casts(FREEZE),
                ME.targets_player(THEM),
                Resolve,
                ME.targets_player(THEM),
                Resolve,
                Resolve,
                THEM.has_cards(Zone::Graveyard, 6),
                THEM.has_cards(Zone::Library, 4),
            ]),
        // The first Brain Freeze's copy was not cast, so the second counts two spells.
        Scenario::new("Brain Freeze does not count copies")
            .battlefield(P0, &[ISLAND, ISLAND, ISLAND, ISLAND, Card::Plains])
            .hand(P0, &[SCIMITAR, FREEZE, FREEZE])
            .library(P1, &[Card::Plains; 20])
            .actions(&[
                ME.casts(SCIMITAR),
                Resolve,
                ME.casts(FREEZE),
                ME.targets_player(THEM),
                Resolve,
                ME.targets_player(THEM),
                Resolve,
                Resolve,
                ME.casts(FREEZE),
                ME.targets_player(THEM),
                Resolve,
                ME.targets_player(THEM),
                ME.targets_player(THEM),
                stack(3),
                Resolve,
                Resolve,
                Resolve,
                THEM.has_cards(Zone::Graveyard, 15),
            ]),
        // Countering one Brain Freeze leaves the other to resolve. The opponent's spell counts
        // for storm, and they keep a second Counterspell, so they hold priority after each spell
        // resolves.
        Scenario::new("Brain Freeze and its copy are countered one at a time")
            .battlefield(P0, &[ISLAND, ISLAND])
            .hand(P0, &[FREEZE])
            .battlefield(P1, &[ISLAND, ISLAND, ISLAND, ISLAND, Card::Plains])
            .hand(P1, &[SCIMITAR, Card::Counterspell, Card::Counterspell])
            .library(P1, &[Card::Plains; 10])
            .at(2, P1, TurnStep::PrecombatMain)
            .actions(&[
                THEM.casts(SCIMITAR),
                Resolve,
                THEM.passes(),
                ME.casts(FREEZE),
                ME.targets_player(THEM),
                Resolve,
                ME.targets_player(THEM),
                THEM.casts(Card::Counterspell),
                THEM.targets(FREEZE),
                Resolve,
                stack(1),
                Resolve,
                THEM.has_cards(Zone::Library, 7),
                ME.has(Zone::Graveyard, FREEZE, 1),
            ]),
    ];
}
