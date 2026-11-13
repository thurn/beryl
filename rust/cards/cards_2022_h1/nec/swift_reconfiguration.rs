// Generated from the card store by `just card header`: notes go below the end marker.
//! Swift Reconfiguration {W}
//! Enchantment — Aura
//!
//! Flash
//! Enchant creature or Vehicle
//! Enchanted permanent is a Vehicle artifact with crew 5 and it loses all other card types. (It's
//! not a creature unless it's crewed.)
// End of the generated card header.

use crate::prelude::*;

/// What a static ability of the Aura does to the permanent it enchants.
const fn enchanted(modification: Modification) -> LayerSpec {
    LayerSpec {
        affected: Objects::AttachedTo,
        condition: None,
        modification,
    }
}

pub static CARD: CardDef = Printed::SwiftReconfiguration
    .keywords(Keywords::FLASH)
    .abilities(&[
        Ability::spell().target(&TargetSpec::permanent(Filter::AnyOf(&[
            Filter::Types(Types::CREATURE),
            Filter::Subtype(Subtype::Vehicle),
        ]))),
        Ability::static_ability(&enchanted(Modification::SetTypes(Types::ARTIFACT))),
        Ability::static_ability(&enchanted(Modification::AddSubtype(Subtype::Vehicle))),
        Ability::static_ability(&enchanted(Modification::grant(0))),
    ])
    .grants(&[Ability::crew(5)])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const SWIFT: &str = Card::SwiftReconfiguration;

    // `Prop::SmallCreature` is the creature to reconfigure: a 2/1, mana value 2. Two
    // `Prop::Creature`, power 3 each, crew 5 together.

    /// A Plains for the Aura, and four Islands for a Thieving Skydiver kicked for up to 2.
    const SKYDIVER_SETUP: Scenario = Scenario::setup()
        .hand(P0, &[SWIFT, Card::ThievingSkydiver])
        .battlefield(
            P0,
            &[
                Card::Plains,
                Card::Island,
                Card::Island,
                Card::Island,
                Card::Island,
            ],
        )
        .battlefield(P1, &[Prop::SmallCreature])
        .library(P0, &[Card::Plains])
        .library(P1, &[Card::Plains]);

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("a creature becomes a noncreature Vehicle that can't crew alone")
            .at(2, P1, TurnStep::PrecombatMain)
            .hand(P0, &[SWIFT])
            .battlefield(P0, &[Card::Plains])
            .battlefield(P1, &[Prop::SmallCreature])
            .hand(P1, &[Card::Plains])
            .library(P0, &[Card::Plains])
            .library(P1, &[Card::Plains, Card::Plains])
            .actions(&[
                THEM.passes(),
                // Flash: cast on the opponent's turn. No one can respond, so it resolves.
                ME.casts(SWIFT),
                THEM.creature(Prop::SmallCreature, false),
                // Crew 5 needs power 5 from other creatures, which P1 doesn't have: only passing
                // and the land are offered.
                THEM.deciding(DecisionKind::Priority, 2),
                ME.controls(SWIFT, 1),
            ]),
        Scenario::new("crewing the Vehicle makes it a creature until end of turn")
            .battlefield(
                P0,
                &[
                    Card::Plains,
                    Prop::SmallCreature,
                    Prop::Creature,
                    Prop::Creature,
                ],
            )
            .hand(P0, &[SWIFT])
            .library(P0, &[Card::Plains, Card::Plains])
            .library(P1, &[Card::Plains, Card::Plains])
            .actions(&[
                ME.casts(SWIFT),
                ME.targets(Prop::SmallCreature),
                Resolve,
                ME.creature(Prop::SmallCreature, false),
                // Crew 5, granted by the Aura: the two others tap, the only way to power 5.
                Action::activate_granted(P0, Prop::SmallCreature, 0),
                ME.tapped(Prop::Creature, 2),
                Resolve,
                ME.creature(Prop::SmallCreature, true),
                ME.pt(Prop::SmallCreature, 2, 1),
                // A creature now, it may attack; it doesn't.
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.done_attacking(),
                // Until end of turn ([CR 702.122a](https://yawgatog.com/resources/magic-rules/#R702122a)).
                advance_to(2, TurnStep::Upkeep),
                ME.creature(Prop::SmallCreature, false),
            ]),
        // The enchanted creature is an artifact whose mana value is still its card's
        // ([CR 202.3](https://yawgatog.com/resources/magic-rules/#R2023)), so a kicked Thieving
        // Skydiver reaches it only with X of at least 2.
        SKYDIVER_SETUP
            .titled("Thieving Skydiver kicked for 1 can't take a reconfigured 2-drop")
            .actions(&[
                ME.casts(SWIFT),
                Resolve,
                Action::Cast {
                    player: P0,
                    card: Card::ThievingSkydiver,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                ME.x(1),
                Resolve,
                // No artifact of mana value 1 or less: the trigger is removed (CR 603.3d).
                stack(0),
                THEM.controls(Prop::SmallCreature, 1),
            ]),
        SKYDIVER_SETUP
            .titled("Thieving Skydiver kicked for 2 takes the reconfigured creature")
            .actions(&[
                ME.casts(SWIFT),
                Resolve,
                Action::Cast {
                    player: P0,
                    card: Card::ThievingSkydiver,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                ME.x(2),
                Resolve,
                Resolve,
                ME.controls(Prop::SmallCreature, 1),
                // The Aura stays its controller's, still attached: the creature is no creature.
                ME.controls(SWIFT, 1),
                ME.creature(Prop::SmallCreature, false),
            ]),
    ];
}
