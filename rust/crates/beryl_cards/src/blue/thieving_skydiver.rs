//! Thieving Skydiver. {1}{U} Creature — Merfolk Rogue, 2/1. "Kicker {X}. X can't be 0. (You
//! may pay an additional {X} as you cast this spell.) Flying. When this creature enters, if
//! it was kicked, gain control of target artifact with mana value X or less. If that artifact
//! is an Equipment, attach it to this creature."

use beryl_engine::card::{
    Ability, CardDef, CastingOption, Colors, Face, Flow, Keywords, ManaCost, Subtype, Subtypes,
    TargetKind, TargetSpec, Trigger, Types,
};
use beryl_engine::effect::{self, EffectCtx};
use beryl_engine::filter::Filter;
use beryl_engine::state::Duration;
use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

use crate::names;

/// "Gain control of target artifact": for as long as the effect lasts, which it doesn't say,
/// so indefinitely ([CR 611.2b](https://yawgatog.com/resources/magic-rules/#R6112b)). No
/// Equipment is among the cards, so attaching never comes up.
fn steal(ctx: &mut EffectCtx) -> Flow {
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
            condition: Some(effect::kicked),
            ..Ability::triggered(Trigger::Enters, steal)
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

pub const SCENARIOS: &[Scenario] = &[
    // Unkicked, two mana, and no trigger.
    Scenario::new("Thieving Skydiver unkicked")
        .hand(P0, &[SKYDIVER])
        .battlefield(P0, ISLANDS.split_at(2).0)
        .actions(&[
            Action::Cast {
                player: P0,
                card: SKYDIVER,
                face: 0,
                costs: 0,
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
                face: 0,
                costs: 1,
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
                pick: beryl_scenario::Pick::Number { n: 2 },
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
