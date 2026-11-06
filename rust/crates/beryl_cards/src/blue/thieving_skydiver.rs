//! Thieving Skydiver. {1}{U} Creature — Merfolk Rogue, 2/1. "Kicker {X}. X can't be 0. (You
//! may pay an additional {X} as you cast this spell.) Flying. When this creature enters, if
//! it was kicked, gain control of target artifact with mana value X or less. If that artifact
//! is an Equipment, attach it to this creature."

use beryl_engine::card::{
    Ability, CardDef, CastingOption, Castings, Colors, Face, FaceIx, Flow, Keywords, ManaCost,
    Subtype, Subtypes, TargetKind, TargetSpec, Trigger, Types,
};
use beryl_engine::effect::{self, EffectCtx};
use beryl_engine::filter::Filter;
use beryl_engine::state::Duration;
use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_engine::{Layout, bodies, conditions};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

use crate::names;

/// "Gain control of target artifact": for as long as the effect lasts, which it doesn't say,
/// so indefinitely ([CR 611.2b](https://yawgatog.com/resources/magic-rules/#R6112b)). No
/// Equipment is among the cards, so attaching never comes up.
fn steal<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    if let Some(artifact) = ctx.target_object(0) {
        ctx.gain_control(artifact, Duration::Indefinite);
    }
    Ok(())
}

pub static CARD: CardDef = CardDef {
    name: names::THIEVING_SKYDIVER,
    faces: &[Face {
        cost: ManaCost::new(1, [0, 1, 0, 0, 0, 0]),
        types: Types::CREATURE,
        subtypes: Subtypes::of(&[Subtype::Merfolk, Subtype::Rogue]),
        colors: Colors::BLUE,
        power: Some(2),
        toughness: Some(1),
        keywords: Keywords::FLYING,
        abilities: &[Ability {
            // X is the kicker's (CR 107.3m), the trigger's own.
            targets: &[TargetSpec::one(
                TargetKind::Permanent,
                Filter::All(&[Filter::Types(Types::ARTIFACT), Filter::ManaValueAtMostX]),
            )],
            condition: Some(conditions!(effect::kicked)),
            ..Ability::triggered(Trigger::Enters, bodies!(steal))
        }],
        casting: &[CastingOption::Kicker {
            cost: ManaCost::X,
            min_x: 1,
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};

const SKYDIVER: &str = names::THIEVING_SKYDIVER;

const ISLANDS: &[Permanent] = &[Permanent::new(names::ISLAND); 4];

const CRYPT: &str = names::MANA_CRYPT;

pub const SCENARIOS: &[Scenario] = &[
    // Get Lost gives the opponent two Maps; kicked for 1, the Skydiver can take a Map or
    // Mana Crypt, each of mana value 0, and takes the Crypt for good.
    Scenario::new("Thieving Skydiver kicked steals Mana Crypt over a Map")
        .hand(P0, &[names::GET_LOST, SKYDIVER])
        .battlefield(
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
        .battlefield(
            P1,
            &[
                Permanent::new(CRYPT),
                Permanent::new(names::ADELINE_RESPLENDENT_CATHAR),
            ],
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
                    card: crate::white::get_lost::MAP_TOKEN,
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
                    card: crate::white::get_lost::MAP_TOKEN,
                    count: 2,
                },
            },
        ]),
    // Unkicked, two mana, and no trigger.
    Scenario::new("Thieving Skydiver unkicked")
        .hand(P0, &[SKYDIVER])
        .battlefield(P0, ISLANDS.split_at(2).0)
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
        .battlefield(P0, ISLANDS)
        .battlefield(P1, &[Permanent::new(names::PLAINS)])
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
