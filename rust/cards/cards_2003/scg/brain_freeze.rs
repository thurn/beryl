// Generated from the card store by `just card header`: notes go below the end marker.
//! Brain Freeze {1}{U}
//! Instant
//!
//! Target player mills three cards.
//! Storm (When you cast this spell, copy it for each spell cast before it this turn. You may choose
//! new targets for the copies.)
// End of the generated card header.

use crate::prelude::*;

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
        // The first Brain Freeze's copy was not cast, so the second counts two spells.
        // Its two copies can choose different targets from each other.
        Scenario::new("Brain Freeze does not count copies")
            .battlefield(P0, &[ISLAND, ISLAND, ISLAND, ISLAND, Card::Plains])
            .hand(P0, &[SCIMITAR, FREEZE, FREEZE])
            .library(P0, &[ISLAND; 4])
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
                ME.targets_player(ME),
                ME.targets_player(THEM),
                stack(3),
                Resolve,
                Resolve,
                Resolve,
                ME.has(Zone::Graveyard, ISLAND, 3),
                THEM.has_cards(Zone::Graveyard, 12),
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
        // The storm trigger exists apart from the spell (CR 113.7a): with Brain Freeze
        // countered under it, the trigger still copies the spell as it last was on the stack.
        Scenario::new("Brain Freeze countered under its storm trigger is still copied")
            .battlefield(P0, &[ISLAND, ISLAND, Card::Plains])
            .hand(P0, &[SCIMITAR, FREEZE])
            .battlefield(P1, &[ISLAND, ISLAND])
            .hand(P1, &[Card::Counterspell])
            .library(P1, &[Card::Plains; 10])
            .actions(&[
                ME.casts(SCIMITAR),
                Resolve,
                ME.casts(FREEZE),
                ME.targets_player(THEM),
                THEM.casts(Card::Counterspell),
                ME.has(Zone::Graveyard, FREEZE, 1),
                stack(1),
                ME.targets_player(THEM),
                Resolve,
                THEM.has_cards(Zone::Library, 7),
            ]),
        // Stifle counters the trigger, so only the original mills cards.
        Scenario::new("countering Brain Freeze's storm trigger prevents every copy")
            .interaction(&[Card::Stifle])
            .battlefield(P0, &[ISLAND, ISLAND, Card::Plains])
            .hand(P0, &[SCIMITAR, FREEZE])
            .battlefield(P1, &[ISLAND])
            .hand(P1, &[Card::Stifle])
            .library(P1, &[Card::Plains; 10])
            .actions(&[
                ME.casts(SCIMITAR),
                Resolve,
                ME.casts(FREEZE),
                ME.targets_player(THEM),
                THEM.casts(Card::Stifle),
                Resolve,
                stack(0),
                THEM.has_cards(Zone::Library, 7),
            ]),
        // The Diamond was cast from the graveyard and countered. Storm still counts
        // it and the Counterspell, so Brain Freeze mills nine cards.
        Scenario::new("Brain Freeze counts a countered spell cast from the graveyard")
            .interaction(&[Card::UnderworldBreach, Card::LionsEyeDiamond])
            .battlefield(P0, &[Card::UnderworldBreach, ISLAND, ISLAND])
            .graveyard(
                P0,
                &[
                    Card::LionsEyeDiamond,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                ],
            )
            .hand(P0, &[FREEZE])
            .battlefield(P1, &[ISLAND, ISLAND])
            .hand(P1, &[Prop::Instant])
            .library(P1, &[Card::Plains; 10])
            .actions(&[
                ME.escapes(Card::LionsEyeDiamond),
                ME.passes(),
                THEM.casts(Prop::Instant),
                Resolve,
                ME.has(Zone::Graveyard, Card::LionsEyeDiamond, 1),
                ME.casts(FREEZE),
                ME.targets_player(THEM),
                Resolve,
                ME.targets_player(THEM),
                ME.targets_player(THEM),
                stack(3),
                Resolve,
                Resolve,
                Resolve,
                THEM.has_cards(Zone::Library, 1),
            ]),
    ];
}
