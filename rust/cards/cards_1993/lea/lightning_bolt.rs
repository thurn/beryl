// Generated from the card store by `just card header`: notes go below the end marker.
//! Lightning Bolt {R}
//! Instant
//!
//! Lightning Bolt deals 3 damage to any target.
// End of the generated card header.

use crate::prelude::*;

/// "Lightning Bolt deals 3 damage to any target." Any target is a creature, a player, a
/// planeswalker or a battle ([CR 115.4]): "These targets may be creatures, players,
/// planeswalkers, or battles." The spell is the source of the damage ([CR 120.2b]): "The spell
/// or ability will specify which object deals that damage." Damage to a player causes that much
/// life loss ([CR 120.3a]), and damage to a creature is marked on it ([CR 120.3e]). If the
/// target is gone when the spell resolves, it has no legal target and does nothing
/// ([CR 608.2b]).
///
/// [CR 115.4]: https://yawgatog.com/resources/magic-rules/#R1154
/// [CR 120.2b]: https://yawgatog.com/resources/magic-rules/#R1202b
/// [CR 120.3a]: https://yawgatog.com/resources/magic-rules/#R1203a
/// [CR 120.3e]: https://yawgatog.com/resources/magic-rules/#R1203e
/// [CR 608.2b]: https://yawgatog.com/resources/magic-rules/#R6082b
pub static CARD: CardDef = Printed::LightningBolt
    .abilities(&[Ability::spell()
        .target(&TargetSpec::any_target())
        .effect(&deal_damage_to_any_target(3))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const BOLT: &str = Card::LightningBolt;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Lightning Bolt deals 3 damage to a player")
            .battlefield(P0, &[Card::Mountain])
            .hand(P0, &[BOLT, Card::Mountain])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                THEM.life(17),
                ME.life(20),
                ME.has(Zone::Graveyard, BOLT, 1),
            ]),
        Scenario::new("Lightning Bolt deals 3 damage to its caster")
            .battlefield(P0, &[Card::Mountain])
            .hand(P0, &[BOLT, Card::Mountain])
            .actions(&[ME.casts(BOLT), ME.targets_player(ME), ME.life(17)]),
        Scenario::new("Lightning Bolt kills a creature")
            .battlefield(P0, &[Card::Mountain])
            .hand(P0, &[BOLT, Card::Mountain])
            .battlefield(P1, &[Prop::Creature])
            .actions(&[
                ME.casts(BOLT),
                ME.targets(Prop::Creature),
                THEM.has(Zone::Battlefield, Prop::Creature, 0),
                THEM.has(Zone::Graveyard, Prop::Creature, 1),
                THEM.life(20),
            ]),
        Scenario::new("Lightning Bolt removes the loyalty of a planeswalker")
            .battlefield(P0, &[Card::Mountain])
            .hand(P0, &[BOLT, Card::Mountain])
            .battlefield(P1, &[Prop::Planeswalker])
            .actions(&[
                ME.casts(BOLT),
                ME.targets(Prop::Planeswalker),
                THEM.has(Zone::Battlefield, Prop::Planeswalker, 0),
                THEM.life(20),
            ]),
        // P0 returns the creature to its owner's hand in response, so the spell's only target
        // is illegal and the spell does nothing.
        Scenario::new("Lightning Bolt does nothing when its creature left in response")
            .battlefield(
                P0,
                &[Card::Mountain, Card::Island, Card::Island, Card::Island],
            )
            .hand(P0, &[BOLT, Prop::Removal])
            .battlefield(P1, &[Prop::Creature])
            .actions(&[
                ME.casts(BOLT),
                ME.targets(Prop::Creature),
                ME.casts(Prop::Removal),
                THEM.has(Zone::Hand, Prop::Creature, 1),
                ME.has(Zone::Graveyard, BOLT, 1),
                THEM.life(20),
            ]),
    ];
}
