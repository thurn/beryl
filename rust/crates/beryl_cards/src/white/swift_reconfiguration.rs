//! Swift Reconfiguration. {W} Enchantment — Aura.
//!
//! "Flash
//! Enchant creature or Vehicle
//! Enchanted permanent is a Vehicle artifact with crew 5 and it loses all other card types.
//! (It's not a creature unless it's crewed.)"

use beryl_engine::card::{
    Ability, AbilityKind, Affected, CardDef, Colors, Face, Keywords, LayerSpec, ManaCost,
    Modification, StaticCondition, Subtype, Subtypes, TargetKind, TargetSpec, Types,
};
use beryl_engine::filter::Filter;

use crate::names;

/// A static ability of the Aura over the permanent it enchants.
const fn enchanted(modification: Modification) -> Ability {
    Ability::static_ability(LayerSpec {
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
            enchanted(Modification::grant(0)),
        ],
        grants: &[Ability::crew(5)],
        ..Face::EMPTY
    }],
    tokens: &[],
};

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_engine::card::{Castings, FaceIx};

    use beryl_engine::{DecisionKind, TurnStep};
    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use crate::{names, props};

    const SWIFT: &str = names::SWIFT_RECONFIGURATION;

    /// The creature to reconfigure: a 2/1, mana value 2.
    const TWO_DROP: &str = props::SMALL_CREATURE;

    const PLAINS: &str = names::PLAINS;

    /// Two of these, power 3 each, crew 5 together.
    const CREW: &str = props::CREATURE;

    const SKYDIVER: &str = names::THIEVING_SKYDIVER;

    /// A Plains for the Aura, and four Islands for a Thieving Skydiver kicked for up to 2.
    const SKYDIVER_LANDS: &[Permanent] = &[
        Permanent::new(PLAINS),
        Permanent::new(names::ISLAND),
        Permanent::new(names::ISLAND),
        Permanent::new(names::ISLAND),
        Permanent::new(names::ISLAND),
    ];

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("a creature becomes a noncreature Vehicle that can't crew alone")
            .at(2, P1, TurnStep::PrecombatMain)
            .hand(P0, &[SWIFT])
            .battlefield(P0, &[Permanent::new(PLAINS)])
            .battlefield(P1, &[Permanent::new(TWO_DROP)])
            .hand(P1, &[PLAINS])
            .library(P0, &[PLAINS])
            .library(P1, &[PLAINS, PLAINS])
            .actions(&[
                Action::pass(P1),
                // Flash: cast on the opponent's turn. No one can respond, so it resolves.
                Action::cast(P0, SWIFT),
                Action::assert(Assertion::Creature {
                    player: P1,
                    card: TWO_DROP,
                    creature: false,
                }),
                // Crew 5 needs power 5 from other creatures, which P1 doesn't have: only passing
                // and the land are offered.
                Action::assert(Assertion::Decision {
                    player: P1,
                    kind: DecisionKind::Priority,
                    candidates: 2,
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
                    Permanent::new(TWO_DROP),
                    Permanent::new(CREW),
                    Permanent::new(CREW),
                ],
            )
            .hand(P0, &[SWIFT])
            .library(P0, &[PLAINS, PLAINS])
            .library(P1, &[PLAINS, PLAINS])
            .actions(&[
                Action::cast(P0, SWIFT),
                Action::choose(P0, DecisionKind::Target, TWO_DROP),
                Action::Resolve,
                Action::assert(Assertion::Creature {
                    player: P0,
                    card: TWO_DROP,
                    creature: false,
                }),
                // Crew 5, granted by the Aura: the two others tap, the only way to power 5.
                Action::activate_granted(P0, TWO_DROP, 0),
                Action::assert(Assertion::Tapped {
                    player: P0,
                    card: CREW,
                    count: 2,
                }),
                Action::Resolve,
                Action::assert(Assertion::Creature {
                    player: P0,
                    card: TWO_DROP,
                    creature: true,
                }),
                Action::assert(Assertion::Pt {
                    player: P0,
                    card: TWO_DROP,
                    token: false,
                    power: 2,
                    toughness: 1,
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
                    card: TWO_DROP,
                    creature: false,
                }),
            ]),
        // The enchanted creature is an artifact whose mana value is still its card's
        // ([CR 202.3](https://yawgatog.com/resources/magic-rules/#R2023)), so a kicked Thieving
        // Skydiver reaches it only with X of at least 2.
        Scenario::new("Thieving Skydiver kicked for 1 can't take a reconfigured 2-drop")
            .interaction(&[SKYDIVER])
            .hand(P0, &[SWIFT, SKYDIVER])
            .battlefield(P0, SKYDIVER_LANDS)
            .battlefield(P1, &[Permanent::new(TWO_DROP)])
            .library(P0, &[PLAINS])
            .library(P1, &[PLAINS])
            .actions(&[
                Action::cast(P0, SWIFT),
                Action::Resolve,
                Action::Cast {
                    player: P0,
                    card: SKYDIVER,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                Action::answer(P0, DecisionKind::X, Pick::Number { n: 1 }),
                Action::Resolve,
                // No artifact of mana value 1 or less: the trigger is removed (CR 603.3d).
                Action::assert(Assertion::Stack { count: 0 }),
                Action::assert(Assertion::Controls {
                    player: P1,
                    card: TWO_DROP,
                    count: 1,
                }),
            ]),
        Scenario::new("Thieving Skydiver kicked for 2 takes the reconfigured creature")
            .interaction(&[SKYDIVER])
            .hand(P0, &[SWIFT, SKYDIVER])
            .battlefield(P0, SKYDIVER_LANDS)
            .battlefield(P1, &[Permanent::new(TWO_DROP)])
            .library(P0, &[PLAINS])
            .library(P1, &[PLAINS])
            .actions(&[
                Action::cast(P0, SWIFT),
                Action::Resolve,
                Action::Cast {
                    player: P0,
                    card: SKYDIVER,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                Action::answer(P0, DecisionKind::X, Pick::Number { n: 2 }),
                Action::Resolve,
                Action::Resolve,
                Action::assert(Assertion::Controls {
                    player: P0,
                    card: TWO_DROP,
                    count: 1,
                }),
                // The Aura stays its controller's, still attached: the creature is no creature.
                Action::assert(Assertion::Controls {
                    player: P0,
                    card: SWIFT,
                    count: 1,
                }),
                Action::assert(Assertion::Creature {
                    player: P0,
                    card: TWO_DROP,
                    creature: false,
                }),
            ]),
    ];
}
