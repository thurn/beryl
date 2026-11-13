// Generated from the card store by `just card header`: notes go below the end marker.
//! Drudge Skeletons {1}{B}
//! Creature — Skeleton, 1/1
//!
//! {B}: Regenerate this creature. (The next time this creature would be destroyed this turn,
//! instead tap it, remove it from combat, and heal all damage on it.)
// End of the generated card header.

use crate::prelude::*;

/// It is in no deck: it was implemented for scenarios that need a creature that regenerates, such
/// as Damn's, whose "can't be regenerated" no other card can tell from a plain destroy.
///
/// "{B}: Regenerate this creature." Each activation that resolves makes one regeneration shield
/// ([CR 701.19a]): "The next time [permanent] would be destroyed this turn, instead remove all
/// damage marked on it and its controller taps it. If it's an attacking or blocking creature,
/// remove it from combat."
///
/// [CR 701.19a]: https://yawgatog.com/resources/magic-rules/#R70119a
pub static CARD: CardDef = Printed::DrudgeSkeletons
    .abilities(&[Ability::activated(Cost::mana(mana("{B}"))).effect(&regenerate(this()))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const DRUDGE: &str = Card::DrudgeSkeletons;

    pub const SCENARIOS: &[Scenario] = &[
        // Blocking P1's 2/1, the Skeletons take lethal damage (CR 704.5g) and regenerate: they
        // stay, tapped. Had the damage stayed marked, the repeated check would destroy them.
        Scenario::new("a shield saves a blocker from lethal combat damage")
            .battlefield(P0, &[Card::Swamp, DRUDGE])
            .battlefield(P1, &[Prop::SmallCreature])
            .library(P0, &[Card::Island, Card::Island])
            .library(P1, &[Card::Island, Card::Island])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.done_attacking(),
                advance_to(2, TurnStep::BeginningOfCombat),
                THEM.passes(),
                ME.passes(),
                THEM.attacks_with(Prop::SmallCreature),
                ME.passes(),
                ME.chooses(DecisionKind::Blocker, DRUDGE),
                ME.activates(DRUDGE, 0),
                advance_to(2, TurnStep::EndOfCombat),
                ME.controls(DRUDGE, 1),
                ME.tapped(DRUDGE, 1),
                THEM.controls(Prop::SmallCreature, 0),
            ]),
        // An attacking Skeletons destroyed by Get Lost regenerates and leaves combat (CR
        // 701.19a), so it deals P1 no combat damage.
        Scenario::new("a shield removes an attacker from combat")
            .interaction(&[Card::GetLost])
            .battlefield(P0, &[Card::Swamp, DRUDGE])
            .battlefield(P1, &[Card::Plains, Card::Plains])
            .hand(P1, &[Card::GetLost])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.activates(DRUDGE, 0),
                Resolve,
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                THEM.passes(),
                ME.attacks_with(DRUDGE),
                THEM.casts(Card::GetLost),
                ME.controls(DRUDGE, 1),
                ME.attacking(DRUDGE, 0),
                advance_to(1, TurnStep::EndOfCombat),
                THEM.life(20),
            ]),
        // One shield replaces one destruction: the second Get Lost of the turn destroys it.
        Scenario::new("a shield is used up by the first destruction")
            .interaction(&[Card::GetLost])
            .battlefield(P0, &[Card::Swamp, DRUDGE])
            .battlefield(
                P1,
                &[Card::Plains, Card::Plains, Card::Plains, Card::Plains],
            )
            .hand(P1, &[Card::GetLost, Card::GetLost])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.activates(DRUDGE, 0),
                Resolve,
                ME.passes(),
                THEM.casts(Card::GetLost),
                Resolve,
                ME.controls(DRUDGE, 1),
                ME.tapped(DRUDGE, 1),
                ME.passes(),
                THEM.casts(Card::GetLost),
                ME.controls(DRUDGE, 0),
                ME.has(Zone::Graveyard, DRUDGE, 1),
            ]),
        // A shield lasts only the turn it was made in (CR 701.19a: "the next time it would be
        // destroyed this turn"), so Get Lost in P1's turn destroys the Skeletons.
        Scenario::new("a shield from last turn is gone")
            .interaction(&[Card::GetLost])
            .battlefield(P0, &[Card::Swamp, DRUDGE])
            .battlefield(P1, &[Card::Plains, Card::Plains])
            .hand(P1, &[Card::GetLost])
            .library(P0, &[Card::Island, Card::Island])
            .library(P1, &[Card::Island, Card::Island])
            .actions(&[
                ME.activates(DRUDGE, 0),
                Resolve,
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                THEM.passes(),
                ME.done_attacking(),
                advance_to(2, TurnStep::PrecombatMain),
                THEM.casts(Card::GetLost),
                Resolve,
                ME.controls(DRUDGE, 0),
                ME.has(Zone::Graveyard, DRUDGE, 1),
            ]),
    ];
}
