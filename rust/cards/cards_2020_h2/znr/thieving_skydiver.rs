// Generated from the card store by `just card header`: notes go below the end marker.
//! Thieving Skydiver {1}{U}
//! Creature — Merfolk Rogue, 2/1
//!
//! Kicker {X}. X can't be 0. (You may pay an additional {X} as you cast this spell.)
//! Flying
//! When this creature enters, if it was kicked, gain control of target artifact with mana value X
//! or less. If that artifact is an Equipment, attach it to this creature.
// End of the generated card header.

use crate::{names, printed};
use beryl_card::effect::build::{TARGET, THIS, attach, if_then, modify, sequence};
use beryl_card::{
    Ability, CardDef, CastingKind, CastingOption, Condition, Duration, Effect, Filter, Keywords,
    ManaCost, Modification, Subtype, TargetKind, TargetSpec, Trigger, Types, card,
};

/// Gains control of the artifact for as long as the effect lasts, which it doesn't say, so
/// indefinitely ([CR 611.2a](https://yawgatog.com/resources/magic-rules/#R6112a)). An
/// Equipment then moves onto the Skydiver, unless it can't equip it
/// ([CR 701.3b](https://yawgatog.com/resources/magic-rules/#R7013b)): "If an effect tries to
/// attach an Aura, Equipment, or Fortification to an object or player it can't be attached
/// to, the Aura, Equipment, or Fortification doesn't move." It still changes control.
static STEAL: Effect = sequence(&[
    modify(TARGET, Modification::Control, Duration::Indefinite),
    if_then(
        Condition::Is(TARGET, Filter::Subtype(Subtype::Equipment)),
        &attach(TARGET, THIS),
    ),
]);

pub static CARD: CardDef = card(
    names::THIEVING_SKYDIVER,
    &[printed::THIEVING_SKYDIVER
        .keywords(Keywords::FLYING)
        .abilities(&[
            // X is the kicker's (CR 107.3m), the trigger's own.
            Ability::triggered(&Trigger::ENTERS)
                .targeting(&[TargetSpec::one(
                    TargetKind::Permanent,
                    Filter::All(&[Filter::Types(Types::ARTIFACT), Filter::ManaValueAtMostX]),
                )])
                .only_if(&Condition::Paid(CastingKind::Kicker))
                .effect(&STEAL),
        ])
        .casting(&[CastingOption::Kicker {
            cost: ManaCost::X,
            min_x: 1,
        }])],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_card::{Castings, FaceIx};

    use beryl_card::{DecisionKind, TurnStep, Zone};

    use beryl_scenario_types::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use crate::{names, props};

    const SKYDIVER: &str = names::THIEVING_SKYDIVER;

    const ISLANDS: &[Permanent] = &[Permanent::new(names::ISLAND); 4];

    const CRYPT: &str = names::MANA_CRYPT;

    pub const SCENARIOS: &[Scenario] = &[
        // Get Lost gives the opponent two Maps; kicked for 1, the Skydiver can take a Map or
        // Mana Crypt, each of mana value 0, and takes the Crypt for good.
        Scenario::new("Thieving Skydiver kicked steals Mana Crypt over a Map")
            .interaction(&[names::MANA_CRYPT, names::GET_LOST])
            .hand(P0, &[names::GET_LOST, SKYDIVER])
            .permanents(
                P0,
                &[
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                ],
            )
            .permanents(
                P1,
                &[Permanent::new(CRYPT), Permanent::new(props::CREATURE)],
            )
            .library(P0, &[names::ISLAND])
            .library(P1, &[names::PLAINS])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: names::GET_LOST,
                    face: FaceIx::FRONT,
                    costs: Castings::NONE,
                },
                Action::Resolve,
                Action::Assert {
                    assertion: Assertion::Controls {
                        player: P1,
                        card: crate::tokens::MAP_TOKEN,
                        count: 2,
                    },
                },
                // Get Lost costs {1}{W}: two Plains, which leaves four lands for the Skydiver.
                Action::Assert {
                    assertion: Assertion::Tapped {
                        player: P0,
                        card: names::PLAINS,
                        count: 2,
                    },
                },
                Action::Cast {
                    player: P0,
                    card: SKYDIVER,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                Action::Answer {
                    player: P0,
                    kind: DecisionKind::X,
                    pick: Pick::Number { n: 1 },
                },
                Action::Resolve,
                // Two Maps and the Crypt: the Maps are one candidate.
                Action::Assert {
                    assertion: Assertion::Decision {
                        player: P0,
                        kind: DecisionKind::Target,
                        candidates: 2,
                    },
                },
                Action::Answer {
                    player: P0,
                    kind: DecisionKind::Target,
                    pick: Pick::Card { card: CRYPT },
                },
                Action::Resolve,
                Action::Assert {
                    assertion: Assertion::Controls {
                        player: P0,
                        card: CRYPT,
                        count: 1,
                    },
                },
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::PrecombatMain,
                },
                // Still P0's after a turn, and its upkeep trigger is P0's now.
                Action::Assert {
                    assertion: Assertion::Controls {
                        player: P0,
                        card: CRYPT,
                        count: 1,
                    },
                },
                Action::Assert {
                    assertion: Assertion::Controls {
                        player: P1,
                        card: crate::tokens::MAP_TOKEN,
                        count: 2,
                    },
                },
            ]),
        // Kicked for 1, the Skydiver takes the opponent's Leonin Scimitar and attaches it to
        // itself: a 2/1 that becomes 3/2.
        Scenario::new("Thieving Skydiver kicked steals an Equipment and wears it")
            .hand(P0, &[SKYDIVER])
            .permanents(P0, ISLANDS)
            .permanents(
                P1,
                &[
                    Permanent::new(props::EQUIPMENT),
                    Permanent::new(props::CREATURE),
                ],
            )
            .library(P0, &[names::ISLAND])
            .library(P1, &[names::PLAINS])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: SKYDIVER,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                Action::Answer {
                    player: P0,
                    kind: DecisionKind::X,
                    pick: Pick::Number { n: 1 },
                },
                Action::Resolve,
                Action::Resolve,
                Action::Assert {
                    assertion: Assertion::Controls {
                        player: P0,
                        card: props::EQUIPMENT,
                        count: 1,
                    },
                },
                Action::Assert {
                    assertion: Assertion::Pt {
                        player: P0,
                        card: SKYDIVER,
                        token: false,
                        power: 3,
                        toughness: 2,
                    },
                },
            ]),
        // Unkicked, two mana, and no trigger.
        Scenario::new("Thieving Skydiver unkicked")
            .hand(P0, &[SKYDIVER])
            .permanents(P0, ISLANDS.split_at(2).0)
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: SKYDIVER,
                    face: FaceIx::FRONT,
                    costs: Castings::NONE,
                },
                Action::Resolve,
                Action::Assert {
                    assertion: Assertion::Stack { count: 0 },
                },
                Action::Assert {
                    assertion: Assertion::Controls {
                        player: P0,
                        card: SKYDIVER,
                        count: 1,
                    },
                },
            ]),
        // Kicked: X is announced from 1, as far as the mana goes; with no artifact to target, the
        // trigger is removed as it would go on the stack (CR 603.3d).
        Scenario::new("Thieving Skydiver kicked with no artifact around")
            .hand(P0, &[SKYDIVER])
            .permanents(P0, ISLANDS)
            .permanents(P1, &[Permanent::new(names::PLAINS)])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: SKYDIVER,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                Action::Assert {
                    assertion: Assertion::Decision {
                        player: P0,
                        kind: DecisionKind::X,
                        candidates: 2,
                    },
                },
                Action::Answer {
                    player: P0,
                    kind: DecisionKind::X,
                    pick: Pick::Number { n: 2 },
                },
                Action::Resolve,
                Action::Assert {
                    assertion: Assertion::Stack { count: 0 },
                },
                Action::Assert {
                    assertion: Assertion::Tapped {
                        player: P0,
                        card: names::ISLAND,
                        count: 4,
                    },
                },
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::End,
                },
                Action::Assert {
                    assertion: Assertion::Count {
                        player: P1,
                        zone: Zone::Battlefield,
                        card: None,
                        count: 1,
                    },
                },
            ]),
    ];
}
