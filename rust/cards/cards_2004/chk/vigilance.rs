// Generated from the card store by `just card header`: notes go below the end marker.
//! Vigilance {W}
//! Enchantment — Aura
//!
//! Enchant creature
//! Enchanted creature has vigilance. (Attacking doesn't cause it to tap.)
// End of the generated card header.

use crate::prelude::*;

/// "Enchanted creature has vigilance" is a static ability that adds a keyword in layer 6
/// ([CR 613.1f]): "Layer 6: Ability-adding effects, ability-removing effects, and effects
/// that say an object can't have an ability are applied." It affects the creature the Aura is
/// attached to ([CR 303.4a]). Attacking does not cause a creature with vigilance to tap
/// ([CR 702.20b]).
///
/// The effect's timestamp is that of the moment the Aura is attached ([CR 613.7e]): a
/// "loses all abilities" effect with a later timestamp takes the keyword away, and one with
/// an earlier timestamp does not.
///
/// [CR 303.4a]: https://yawgatog.com/resources/magic-rules/#R3034a
/// [CR 613.1f]: https://yawgatog.com/resources/magic-rules/#R6131f
/// [CR 613.7e]: https://yawgatog.com/resources/magic-rules/#R6137e
/// [CR 702.20b]: https://yawgatog.com/resources/magic-rules/#R70220b
pub static CARD: CardDef = Printed::Vigilance
    .abilities(&[
        Ability::spell().target(&TargetSpec::creature()),
        Ability::static_ability(&LayerSpec {
            affected: Objects::AttachedTo,
            condition: None,
            modifications: &[Modification::AddKeywords(Keywords::VIGILANCE)],
        }),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const VIGILANCE: &str = Card::Vigilance;
    const DRESS_DOWN: &str = Card::DressDown;

    // `Prop::Creature` is Elite Spellbinder, a 3/1 flier with no vigilance of its own.
    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("an enchanted creature attacks without tapping")
            .battlefield(P0, &[Card::Plains, Prop::Creature])
            .hand(P0, &[VIGILANCE])
            .actions(&[
                ME.casts(VIGILANCE),
                Resolve,
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(Prop::Creature),
                ME.tapped(Prop::Creature, 0),
            ]),
        Scenario::new("a creature without the Aura taps to attack")
            .battlefield(P0, &[Prop::Creature])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(Prop::Creature),
                ME.tapped(Prop::Creature, 1),
            ]),
        // "Loses all abilities" began before the Aura was attached, so its timestamp is the
        // earlier and the keyword the Aura grants stays.
        Scenario::new("an Aura attached after Dress Down still gives vigilance")
            .battlefield(P0, &[DRESS_DOWN, Card::Plains, Prop::Creature])
            .hand(P0, &[VIGILANCE])
            .actions(&[
                ME.casts(VIGILANCE),
                Resolve,
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(Prop::Creature),
                ME.tapped(Prop::Creature, 0),
            ]),
        // Dress Down, cast with flash after the Aura was attached, has the later timestamp:
        // the creature loses vigilance, though the Aura stays attached.
        Scenario::new("Dress Down after the Aura takes vigilance away")
            .battlefield(
                P0,
                &[Card::Plains, Card::Island, Card::Island, Prop::Creature],
            )
            .hand(P0, &[VIGILANCE, DRESS_DOWN])
            .library(P0, &[Card::Plains, Card::Island])
            .actions(&[
                ME.casts(VIGILANCE),
                Resolve,
                ME.casts(DRESS_DOWN),
                Resolve,
                stack(1),
                Resolve,
                ME.controls(VIGILANCE, 1),
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(Prop::Creature),
                ME.tapped(Prop::Creature, 1),
            ]),
    ];
}
