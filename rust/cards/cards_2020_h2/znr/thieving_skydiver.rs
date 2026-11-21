// Generated from the card store by `just card header`: notes go below the end marker.
//! Thieving Skydiver {1}{U}
//! Creature — Merfolk Rogue, 2/1
//!
//! Kicker {X}. X can't be 0. (You may pay an additional {X} as you cast this spell.)
//! Flying
//! When this creature enters, if it was kicked, gain control of target artifact with mana value X
//! or less. If that artifact is an Equipment, attach it to this creature.
// End of the generated card header.

use crate::prelude::*;

/// Skydiver's enters trigger gets X from the kicker paid for the spell. Under
/// [CR 107.3m], “the value of X for that ability is the same as the value of X for
/// that spell,” even though X for the permanent is zero.
///
/// An Equipment changes control even if it cannot attach to the Skydiver. [CR 701.3b]
/// says “the Aura, Equipment, or Fortification doesn't move” when it cannot attach.
///
/// [CR 107.3m]: https://yawgatog.com/resources/magic-rules/#R1073m
/// [CR 701.3b]: https://yawgatog.com/resources/magic-rules/#R7013b
pub static CARD: CardDef = Printed::ThievingSkydiver
    .keywords(Keywords::FLYING)
    .abilities(&[Ability::triggered(&Trigger::ENTERS)
        .target(&TargetSpec::permanent(Filter::All(&[
            Filter::Types(Types::ARTIFACT),
            Filter::ManaValueAtMostX,
        ])))
        .only_if(&Condition::Paid(CastingKind::Kicker))
        .effect(&sequence(&[
            modify(target(), &[Modification::Control], Duration::Indefinite),
            if_then(
                Condition::Is(target(), Filter::Subtype(Subtype::Equipment)),
                &attach(target(), this()),
            ),
        ]))])
    .casting(&[CastingOption::Kicker {
        cost: ManaCost::X,
        min_x: 1,
    }])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const SKYDIVER: &str = Card::ThievingSkydiver;

    pub const SCENARIOS: &[Scenario] = &[
        // Get Lost gives the opponent two Maps; kicked for 1, the Skydiver can take a Map or
        // Mana Crypt, each of mana value 0, and takes the Crypt for good.
        Scenario::new("Thieving Skydiver kicked steals Mana Crypt over a Map")
            .interaction(&[Card::ManaCrypt, Card::GetLost])
            .hand(P0, &[Card::GetLost, SKYDIVER])
            .battlefield(
                P0,
                &[
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .battlefield(P1, &[Card::ManaCrypt, Prop::Flier])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.casts(Card::GetLost),
                Resolve,
                THEM.controls(crate::tokens::MAP_TOKEN, 2),
                // Get Lost costs {1}{W}: two Plains, which leaves four lands for the Skydiver.
                ME.tapped(Card::Plains, 2),
                Action::Cast {
                    player: P0,
                    card: SKYDIVER,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                ME.x(1),
                Resolve,
                // Two Maps and the Crypt: the Maps are one candidate.
                ME.deciding(DecisionKind::Target, 2),
                ME.targets(Card::ManaCrypt),
                Resolve,
                ME.controls(Card::ManaCrypt, 1),
                advance_to(3, TurnStep::PrecombatMain),
                // Still P0's after a turn, and its upkeep trigger is P0's now.
                ME.controls(Card::ManaCrypt, 1),
                THEM.controls(crate::tokens::MAP_TOKEN, 2),
            ]),
    ];
}
