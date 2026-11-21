// Generated from the card store by `just card header`: notes go below the end marker.
//! Damn {B}{B}
//! Sorcery
//!
//! Destroy target creature. A creature destroyed this way can't be regenerated.
//! Overload {2}{W}{W} (You may cast this spell for its overload cost. If you do, change "target" in
//! its text to "each.")
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::Damn
    .abilities(&[
        Ability::spell()
            .target(&TargetSpec::creature())
            .effect(&destroy_without_regeneration(target())),
        Ability::spell().effect(&destroy_without_regeneration(Objects::All {
            zone: Zone::Battlefield,
            filter: Filter::Types(Types::CREATURE),
        })),
    ])
    .casting(&[CastingOption::overload(mana("{2}{W}{W}"), 1)])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;
    use crate::tokens::HUMAN_TOKEN;

    pub const SCENARIOS: &[Scenario] = &[
        // For {B}{B}, one target: P0 destroys P1's flier and keeps its own two-drop.
        Scenario::new("Damn for {B}{B} destroys its target alone")
            .battlefield(P0, &[Card::Swamp, Card::Swamp, Prop::Vanilla])
            .hand(P0, &[Card::Damn])
            .battlefield(P1, &[Prop::Flier])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.casts(Card::Damn),
                ME.targets(Prop::Flier),
                Resolve,
                THEM.controls(Prop::Flier, 0),
                ME.controls(Prop::Vanilla, 1),
            ]),
        // With only white mana, the overload is the one way to cast it: two candidates, passing
        // and the overloaded cast. It asks for no target and destroys every creature, P0's own
        // among them.
        Scenario::new("Damn overloaded destroys each creature, targeting none")
            .battlefield(
                P0,
                &[
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Prop::Vanilla,
                ],
            )
            .hand(P0, &[Card::Damn])
            .battlefield(P1, &[Prop::Flier, Prop::Vanilla])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.deciding(DecisionKind::Priority, 2),
                Action::Cast {
                    player: P0,
                    card: Card::Damn,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                Resolve,
                ME.controls(Prop::Vanilla, 0),
                THEM.controls(Prop::Flier, 0),
                THEM.controls(Prop::Vanilla, 0),
                THEM.has_cards(Zone::Graveyard, 2),
                ME.has_cards(Zone::Graveyard, 2),
            ]),
        // Overloaded, it takes every kind of creature in one move: a token, which ceases to exist
        // once in the graveyard ([CR 704.5d](https://yawgatog.com/resources/magic-rules/#R7045d):
        // "If a token is in a zone other than the battlefield, it ceases to exist"), and P1's
        // face-down creature, revealed as it leaves
        // ([CR 708.9](https://yawgatog.com/resources/magic-rules/#R7089): "If a face-down
        // permanent ... moves from the battlefield to any other zone, its owner must reveal it").
        // Adeline's Human is the token, and a token's maker is on the battlefield with it.
        Scenario::new("Damn overloaded takes tokens and face-down creatures at once")
            .interaction(&[Card::AdelineResplendentCathar])
            .battlefield(
                P0,
                &[
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::AdelineResplendentCathar,
                ],
            )
            .permanents(P0, &[Permanent::token(HUMAN_TOKEN)])
            .hand(P0, &[Card::Damn])
            .battlefield(P1, &[Prop::Flier])
            .permanents(P1, &[Permanent::new(Prop::Vanilla).face_down()])
            .library(P1, &[Card::Island])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: Card::Damn,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                Resolve,
                ME.controls(Card::AdelineResplendentCathar, 0),
                ME.controls(HUMAN_TOKEN, 0),
                THEM.controls(Prop::Flier, 0),
                ME.has_cards(Zone::Graveyard, 2),
                THEM.has(Zone::Graveyard, Prop::Vanilla, 1),
            ]),
        // A creature with a regeneration shield is still destroyed: Damn's "can't be regenerated"
        // keeps the shield from applying (CR 701.19c). Drudge Skeletons is the creature that
        // regenerates.
        Scenario::new("Damn for {B}{B} destroys a creature with a regeneration shield")
            .interaction(&[Card::DrudgeSkeletons])
            .battlefield(
                P0,
                &[Card::Swamp, Card::Swamp, Card::Swamp, Card::DrudgeSkeletons],
            )
            .hand(P0, &[Card::Damn])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.activates(Card::DrudgeSkeletons, 0),
                Resolve,
                ME.casts(Card::Damn),
                ME.controls(Card::DrudgeSkeletons, 0),
                ME.has(Zone::Graveyard, Card::DrudgeSkeletons, 1),
            ]),
        // Overloaded, the same: P1 regenerates its Skeletons when P0 passes priority, and Damn
        // destroys them.
        Scenario::new("Damn overloaded destroys a creature with a regeneration shield")
            .interaction(&[Card::DrudgeSkeletons])
            .battlefield(
                P0,
                &[Card::Plains, Card::Plains, Card::Plains, Card::Plains],
            )
            .hand(P0, &[Card::Damn])
            .battlefield(P1, &[Card::Swamp, Card::DrudgeSkeletons])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.passes(),
                THEM.activates(Card::DrudgeSkeletons, 0),
                Action::Cast {
                    player: P0,
                    card: Card::Damn,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                THEM.controls(Card::DrudgeSkeletons, 0),
                THEM.has(Zone::Graveyard, Card::DrudgeSkeletons, 1),
            ]),
    ];
}
