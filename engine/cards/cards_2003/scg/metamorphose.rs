// Generated from the card store by `just card header`: notes go below the end marker.
//! Metamorphose {1}{U}
//! Instant
//!
//! Put target permanent an opponent controls on top of its owner's library. That opponent may put
//! an artifact, creature, enchantment, or land card from their hand onto the battlefield.
// End of the generated card header.

use crate::prelude::*;

/// "That opponent" is the player who controlled the permanent, read as it last existed on
/// the battlefield, who need not be its owner ([CR 608.2h]).
///
/// [CR 608.2h]: https://yawgatog.com/resources/magic-rules/#R6082h
pub static CARD: CardDef = Printed::Metamorphose
    .abilities(&[Ability::spell()
        .target(&TargetSpec::permanent(Filter::Whose(
            &Players::EachOpponent,
        )))
        .effect(&sequence(&[
            put_into_library(target(), LibraryPosition::Top),
            may_choose_hidden(
                its_controller(),
                cards_in(
                    Zone::Hand,
                    &its_controller(),
                    Filter::Types(
                        Types::ARTIFACT
                            .union(Types::CREATURE)
                            .union(Types::ENCHANTMENT)
                            .union(Types::LAND),
                    ),
                ),
                Prompt::new(Purpose::Look)
                    .most(1)
                    .places(Place::Battlefield, Place::None),
            ),
            put_onto_battlefield(&PutOnto {
                objects: those(),
                under: None,
                tapped: false,
                linked: false,
            }),
        ]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const META: &str = Card::Metamorphose;

    pub const SCENARIOS: &[Scenario] = &[
        // P0's Control Magic took P1's creature. P1's Metamorphose puts it on top of P1's own
        // library, and "that opponent" is P0, who controlled it, not P1, who owns it: P0 may
        // put a card from P0's hand.
        Scenario::new("Metamorphose on a stolen creature: its controller puts a card")
            .interaction(&[Card::ControlMagic, Card::BloodMoon])
            .hand(P0, &[Card::ControlMagic, Prop::Artifact])
            .battlefield(P0, &[Card::Island; 4])
            .hand(P1, &[META, Card::BloodMoon])
            .battlefield(P1, &[Card::Island, Card::Island, Prop::Vanilla])
            .actions(&[
                ME.casts(Card::ControlMagic),
                Resolve,
                ME.controls(Prop::Vanilla, 1),
                ME.passes(),
                THEM.casts(META),
                THEM.targets(Prop::Vanilla),
                ME.picks_up_to(Prop::Artifact),
                ME.controls(Prop::Artifact, 1),
                THEM.controls(Prop::Vanilla, 0),
                ME.controls(Prop::Vanilla, 0),
                THEM.has(Zone::Hand, Card::BloodMoon, 1),
                ME.has(Zone::Graveyard, Card::ControlMagic, 1),
            ]),
        // An instant is no artifact, creature, enchantment or land card: P1 may put only the
        // artifact.
        Scenario::new("Metamorphose: the opponent can't put an instant")
            .hand(P0, &[META])
            .battlefield(P0, &[Card::Island, Card::Island])
            .battlefield(P1, &[Prop::Vanilla])
            .hand(P1, &[Prop::Instant, Prop::Artifact])
            .actions(&[
                ME.casts(META),
                Resolve,
                THEM.deciding(DecisionKind::ChooseUpTo, 2),
                THEM.picks_up_to(Prop::Artifact),
                THEM.controls(Prop::Artifact, 1),
                THEM.controls(Prop::Vanilla, 0),
                THEM.has(Zone::Hand, Prop::Instant, 1),
            ]),
    ];
}
