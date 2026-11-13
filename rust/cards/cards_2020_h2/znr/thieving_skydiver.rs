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

/// Gains control of the artifact for as long as the effect lasts, which it doesn't say, so
/// indefinitely ([CR 611.2a](https://yawgatog.com/resources/magic-rules/#R6112a)). An
/// Equipment then moves onto the Skydiver, unless it can't equip it
/// ([CR 701.3b](https://yawgatog.com/resources/magic-rules/#R7013b)): "If an effect tries to
/// attach an Aura, Equipment, or Fortification to an object or player it can't be attached
/// to, the Aura, Equipment, or Fortification doesn't move." It still changes control.
pub static CARD: CardDef = Printed::ThievingSkydiver
    .keywords(Keywords::FLYING)
    .abilities(&[
        // X is the kicker's (CR 107.3m), the trigger's own.
        Ability::triggered(&Trigger::ENTERS)
            .target(&TargetSpec::permanent(Filter::All(&[
                Filter::Types(Types::ARTIFACT),
                Filter::ManaValueAtMostX,
            ])))
            .only_if(&Condition::Paid(CastingKind::Kicker))
            .effect(&sequence(&[
                modify(target(), Modification::Control, Duration::Indefinite),
                if_then(
                    Condition::Is(target(), Filter::Subtype(Subtype::Equipment)),
                    &attach(target(), this()),
                ),
            ])),
    ])
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
            .battlefield(P1, &[Card::ManaCrypt, Prop::Creature])
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
        // Kicked for 1, the Skydiver takes the opponent's Leonin Scimitar and attaches it to
        // itself: a 2/1 that becomes 3/2.
        Scenario::new("Thieving Skydiver kicked steals an Equipment and wears it")
            .hand(P0, &[SKYDIVER])
            .battlefield(P0, &[Card::Island; 4])
            .battlefield(P1, &[Prop::Equipment, Prop::Creature])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Plains])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: SKYDIVER,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                ME.x(1),
                Resolve,
                Resolve,
                ME.controls(Prop::Equipment, 1),
                ME.pt(SKYDIVER, 3, 2),
            ]),
        // Unkicked, two mana, and no trigger.
        Scenario::new("Thieving Skydiver unkicked")
            .hand(P0, &[SKYDIVER])
            .battlefield(P0, &[Card::Island; 2])
            .actions(&[
                ME.casts(SKYDIVER),
                Resolve,
                stack(0),
                ME.controls(SKYDIVER, 1),
            ]),
        // Kicked: X is announced from 1, as far as the mana goes; with no artifact to target, the
        // trigger is removed as it would go on the stack (CR 603.3d).
        Scenario::new("Thieving Skydiver kicked with no artifact around")
            .hand(P0, &[SKYDIVER])
            .battlefield(P0, &[Card::Island; 4])
            .battlefield(P1, &[Card::Plains])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: SKYDIVER,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                ME.deciding(DecisionKind::X, 2),
                ME.x(2),
                Resolve,
                stack(0),
                ME.tapped(Card::Island, 4),
                advance_to(1, TurnStep::End),
                THEM.has_cards(Zone::Battlefield, 1),
            ]),
    ];
}
