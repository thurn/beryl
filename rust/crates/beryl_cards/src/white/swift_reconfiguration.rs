//! Swift Reconfiguration. {W} Enchantment — Aura.
//!
//! "Flash
//! Enchant creature or Vehicle
//! Enchanted permanent is a Vehicle artifact with crew 5 and it loses all other card types.
//! (It's not a creature unless it's crewed.)"

use beryl_engine::card::{
    Ability, AbilityKind, Affected, CardDef, Colors, Face, GRANTED, Keywords, LayerSpec, ManaCost,
    Modification, StaticCondition, Subtype, Subtypes, TargetKind, TargetSpec, Types,
};
use beryl_engine::filter::Filter;
use beryl_engine::{DecisionKind, TurnStep};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

use crate::names;

/// A static ability of the Aura over the permanent it enchants.
const fn enchanted(modification: Modification) -> Ability {
    Ability::statik(LayerSpec {
        affected: Affected::Enchanted,
        condition: StaticCondition::Always,
        modification,
    })
}

pub static CARD: CardDef = CardDef {
    name: names::SWIFT_RECONFIGURATION,
    faces: &[Face {
        cost: ManaCost::new(0, [1, 0, 0, 0, 0, 0]),
        types: Types::ENCHANTMENT,
        subtypes: Subtypes::of(&[Subtype::Aura]),
        colors: Colors::WHITE,
        keywords: Keywords::FLASH,
        abilities: &[
            // Enchant creature or Vehicle: the Aura spell's target
            // ([CR 303.4a](https://yawgatog.com/resources/magic-rules/#R3034a)): "An Aura
            // spell requires a target, which is defined by its enchant ability." It resolves
            // attached to it (CR 608.3c).
            Ability {
                kind: AbilityKind::Spell,
                targets: &[TargetSpec::one(
                    TargetKind::Permanent,
                    Filter::AnyOf(&[
                        Filter::Types(Types::CREATURE),
                        Filter::Subtype(Subtype::Vehicle),
                    ]),
                )],
                ..Ability::NONE
            },
            // Layer 4 ([CR 205.1a](https://yawgatog.com/resources/magic-rules/#R2051a)): the
            // new card type replaces the others, and the subtypes of the types it loses go
            // with them.
            enchanted(Modification::SetTypes(Types::ARTIFACT)),
            enchanted(Modification::AddSubtype(Subtype::Vehicle)),
            // Layer 6: "with crew 5".
            enchanted(Modification::Grant(0)),
        ],
        grants: &[Ability::crew(5)],
        ..Face::EMPTY
    }],
    tokens: &[],
};

const SWIFT: &str = names::SWIFT_RECONFIGURATION;
const JACE: &str = names::JACE_VRYNS_PRODIGY;
const PLAINS: &str = names::PLAINS;
const OVERLORD: &str = names::OVERLORD_OF_THE_MISTMOORS;

pub const SCENARIOS: &[Scenario] = &[
    Scenario::new("a creature becomes a noncreature Vehicle that can't crew alone")
        .at(2, P1, TurnStep::PrecombatMain)
        .hand(P0, &[SWIFT])
        .battlefield(P0, &[Permanent::new(PLAINS)])
        .battlefield(P1, &[Permanent::new(JACE)])
        .library(P0, &[PLAINS])
        .library(P1, &[PLAINS, PLAINS])
        .actions(&[
            Action::pass(P1),
            // Flash: cast on the opponent's turn.
            Action::cast(P0, SWIFT),
            Action::Resolve,
            Action::assert(Assertion::Creature {
                player: P1,
                card: JACE,
                creature: false,
            }),
            // Not a creature, so its {T} ability isn't summoning-sick-limited, but crew 5 needs
            // power 5 from other creatures, which P1 doesn't have: only its loot ability is offered.
            Action::assert(Assertion::Decision {
                player: P1,
                kind: DecisionKind::Priority,
                candidates: 2,
            }),
            // Its loot resolves unasked, with Plains alone to discard.
            Action::activate(P1, JACE, 0),
            Action::assert(Assertion::Count {
                player: P1,
                zone: beryl_engine::Zone::Graveyard,
                card: Some(PLAINS),
                count: 1,
            }),
            Action::assert(Assertion::Controls {
                player: P0,
                card: SWIFT,
                count: 1,
            }),
        ]),
    Scenario::new("crewing the Vehicle makes it a creature until end of turn")
        .battlefield(
            P0,
            &[
                Permanent::new(PLAINS),
                Permanent::new(JACE),
                Permanent::new(OVERLORD),
            ],
        )
        .hand(P0, &[SWIFT])
        .library(P0, &[PLAINS, PLAINS])
        .library(P1, &[PLAINS, PLAINS])
        .actions(&[
            Action::cast(P0, SWIFT),
            Action::choose(P0, DecisionKind::Target, JACE),
            Action::Resolve,
            Action::assert(Assertion::Creature {
                player: P0,
                card: JACE,
                creature: false,
            }),
            // Crew 5, granted by the Aura: the Overlord taps, the only creature that can.
            Action::activate(P0, JACE, GRANTED),
            Action::assert(Assertion::Tapped {
                player: P0,
                card: OVERLORD,
                count: 1,
            }),
            Action::Resolve,
            Action::assert(Assertion::Creature {
                player: P0,
                card: JACE,
                creature: true,
            }),
            Action::assert(Assertion::Pt {
                player: P0,
                card: JACE,
                token: false,
                power: 0,
                toughness: 2,
            }),
            // A creature now, it may attack; it doesn't.
            Action::AdvanceTo {
                turn: 1,
                step: TurnStep::BeginningOfCombat,
            },
            Action::pass(P0),
            Action::answer(P0, DecisionKind::Attacker, Pick::Done),
            // Until end of turn ([CR 702.122a](https://yawgatog.com/resources/magic-rules/#R702122a)).
            Action::AdvanceTo {
                turn: 2,
                step: TurnStep::Upkeep,
            },
            Action::assert(Assertion::Creature {
                player: P0,
                card: JACE,
                creature: false,
            }),
        ]),
];
