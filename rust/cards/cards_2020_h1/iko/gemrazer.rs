// Generated from the card store by `just card header`: notes go below the end marker.
//! Gemrazer {3}{G}
//! Creature — Beast, 4/4
//!
//! Mutate {1}{G}{G} (If you cast this spell for its mutate cost, put it over or under target
//! non-Human creature you own. They mutate into the creature on top plus all abilities from under
//! it.)
//! Reach, trample
//! Whenever this creature mutates, destroy target artifact or enchantment an opponent controls.
// End of the generated card header.

use crate::prelude::*;

/// Mutate is an alternative cost ([CR 702.140a]); the spell ability at index 1 is what only
/// the mutating creature spell has: its target and its merge ([`Effect::Mutate`]). Cast for
/// its mana cost, it is a plain creature spell with no target. Its triggered ability
/// triggers for the merged permanent whether Gemrazer went over or under (CR 702.140d).
///
/// [CR 702.140a]: https://yawgatog.com/resources/magic-rules/#R702140a
pub static CARD: CardDef = Printed::Gemrazer
    .keywords(Keywords::REACH.union(Keywords::TRAMPLE))
    .abilities(&[
        Ability::triggered(&Trigger::MUTATES)
            .target(&TargetSpec::permanent(Filter::All(&[
                Filter::Types(Types::ARTIFACT.union(Types::ENCHANTMENT)),
                Filter::Whose(&Players::EachOpponent),
            ])))
            .effect(&destroy(target())),
        Ability::spell()
            .target(&TargetSpec::permanent(Filter::All(&[
                Filter::Types(Types::CREATURE),
                Filter::Not(&Filter::Subtype(Subtype::Human)),
                Filter::Owned(&Players::You),
            ])))
            .effect(&Effect::Mutate(target())),
    ])
    .casting(&[CastingOption::mutate(mana("{1}{G}{G}"), 1)])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    /// The cast for the mutate cost, its first option.
    const MUTATE: Action = Action::Cast {
        player: P0,
        card: Card::Gemrazer,
        face: FaceIx::FRONT,
        costs: Castings::only(0),
    };

    pub const SCENARIOS: &[Scenario] = &[
        // Under, the merged creature is Blood Artist, a 0/1 Vampire, with Gemrazer's abilities:
        // its "whenever this creature mutates" triggers and destroys P1's Scimitar. Gemrazer's
        // card is on the battlefield as part of it, and no creature entered.
        Scenario::new("Gemrazer under a creature: the creature's values, Gemrazer's abilities")
            .battlefield(
                P0,
                &[Card::Forest, Card::Forest, Card::Forest, Card::BloodArtist],
            )
            .hand(P0, &[Card::Gemrazer])
            .battlefield(P1, &[Card::LeoninScimitar])
            .library(P1, &[Card::Island])
            .actions(&[
                MUTATE,
                Resolve,
                ME.no(),
                Resolve,
                THEM.controls(Card::LeoninScimitar, 0),
                THEM.has(Zone::Graveyard, Card::LeoninScimitar, 1),
                ME.controls(Card::BloodArtist, 1),
                ME.pt(Card::BloodArtist, 0, 1),
                ME.controls(Card::Gemrazer, 0),
                ME.has_cards(Zone::Hand, 0),
                ME.has_cards(Zone::Graveyard, 0),
            ]),
        // Over, it is a 4/4 Gemrazer, and its trigger is Gemrazer's own.
        Scenario::new("Gemrazer over a creature: a 4/4 Gemrazer that mutated")
            .battlefield(
                P0,
                &[Card::Forest, Card::Forest, Card::Forest, Card::BloodArtist],
            )
            .hand(P0, &[Card::Gemrazer])
            .battlefield(P1, &[Card::LeoninScimitar])
            .library(P1, &[Card::Island])
            .actions(&[
                MUTATE,
                Resolve,
                ME.yes(),
                Resolve,
                THEM.controls(Card::LeoninScimitar, 0),
                ME.controls(Card::Gemrazer, 1),
                ME.pt(Card::Gemrazer, 4, 4),
                ME.controls(Card::BloodArtist, 0),
            ]),
        // The merged creature dies: one creature died, so Blood Artist's "whenever this creature
        // or another creature dies", from under Gemrazer, triggers once (CR 731.3), and both
        // cards go to P0's graveyard.
        Scenario::new("Gemrazer's merged creature dies: both cards go, a dies trigger once")
            .battlefield(
                P0,
                &[Card::Forest, Card::Forest, Card::Forest, Card::BloodArtist],
            )
            .hand(P0, &[Card::Gemrazer])
            .battlefield(P1, &[Card::Plains, Card::Plains, Card::LeoninScimitar])
            .hand(P1, &[Card::GetLost])
            .library(P1, &[Card::Island])
            .actions(&[
                MUTATE,
                Resolve,
                ME.yes(),
                Resolve,
                ME.passes(),
                THEM.casts(Card::GetLost),
                Resolve,
                ME.targets_player(THEM),
                Resolve,
                ME.controls(Card::Gemrazer, 0),
                ME.has(Zone::Graveyard, Card::Gemrazer, 1),
                ME.has(Zone::Graveyard, Card::BloodArtist, 1),
                THEM.life(19),
                ME.life(21),
            ]),
        // Bounced, the merged creature's cards both go to P0's hand.
        Scenario::new("Gemrazer's merged creature bounced: both cards to hand")
            .battlefield(
                P0,
                &[Card::Forest, Card::Forest, Card::Forest, Card::BloodArtist],
            )
            .hand(P0, &[Card::Gemrazer])
            .battlefield(
                P1,
                &[
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::LeoninScimitar,
                ],
            )
            .hand(P1, &[Card::SinkIntoStupor])
            .library(P1, &[Card::Island])
            .actions(&[
                MUTATE,
                Resolve,
                ME.no(),
                Resolve,
                ME.passes(),
                THEM.casts(Card::SinkIntoStupor),
                ME.controls(Card::BloodArtist, 0),
                ME.has(Zone::Hand, Card::Gemrazer, 1),
                ME.has(Zone::Hand, Card::BloodArtist, 1),
            ]),
        // The target dies in response: the spell is no longer a mutating creature spell and
        // enters as a 4/4 Gemrazer (CR 702.140b), which did not mutate, so no trigger.
        Scenario::new("Gemrazer's target gone: it enters as a creature and does not mutate")
            .battlefield(
                P0,
                &[Card::Forest, Card::Forest, Card::Forest, Card::BloodArtist],
            )
            .hand(P0, &[Card::Gemrazer])
            .battlefield(P1, &[Card::Plains, Card::Plains, Card::LeoninScimitar])
            .hand(P1, &[Card::GetLost])
            .library(P1, &[Card::Island])
            .actions(&[
                MUTATE,
                THEM.casts(Card::GetLost),
                Resolve,
                ME.targets_player(THEM),
                Resolve,
                Resolve,
                ME.controls(Card::Gemrazer, 1),
                ME.pt(Card::Gemrazer, 4, 4),
                THEM.controls(Card::LeoninScimitar, 1),
                stack(0),
            ]),
        // Phantasmal Image copies the merged creature: its copiable values are Gemrazer's with
        // every ability of Blood Artist (CR 731.2a, CR 707.2), so the copy has Blood Artist's
        // ability too. When the merged creature dies, both Blood Artist abilities trigger:
        // the one under the merged creature and the copy's.
        Scenario::new(
            "Phantasmal Image copying Gemrazer's merged creature has both cards' abilities",
        )
        .interaction(&[Card::PhantasmalImage])
        .battlefield(
            P0,
            &[
                Card::Forest,
                Card::Forest,
                Card::Forest,
                Card::Island,
                Card::Island,
                Card::BloodArtist,
            ],
        )
        .hand(P0, &[Card::Gemrazer, Card::PhantasmalImage])
        .battlefield(P1, &[Card::Plains, Card::Plains, Card::LeoninScimitar])
        .hand(P1, &[Card::GetLost])
        .library(P1, &[Card::Island])
        .actions(&[
            MUTATE,
            Resolve,
            ME.yes(),
            Resolve,
            ME.casts(Card::PhantasmalImage),
            Resolve,
            ME.picks_up_to(Card::Gemrazer),
            ME.controls(Card::Gemrazer, 2),
            THEM.casts(Card::GetLost),
            THEM.targets(Card::Gemrazer),
            Resolve,
            ME.chooses(DecisionKind::TriggerOrder, Card::Gemrazer),
            ME.targets_player(THEM),
            ME.targets_player(THEM),
            Resolve,
            Resolve,
            ME.controls(Card::Gemrazer, 1),
            THEM.life(18),
            ME.life(22),
        ]),
    ];
}
