// Generated from the card store by `just card header`: notes go below the end marker.
//! Mending Hands {W}
//! Instant
//!
//! Prevent the next 4 damage that would be dealt to any target this turn.
// End of the generated card header.

use crate::prelude::*;

/// It is in no deck: it was implemented for scenarios that need a prevention shield of an
/// amount, such as Furnace of Rath's, where the order of a shield and a doubling changes how much
/// damage is dealt.
///
/// "Prevent the next 4 damage that would be dealt to any target this turn." A shield of an amount
/// ([CR 615.7]): "Each 1 damage that would be dealt to the “shielded” permanent or player is
/// prevented. Preventing 1 damage reduces the remaining shield by 1." It lasts until it is used up
/// or the turn ends ([CR 615.3]): "Such effects last until they're used up or their duration has
/// expired."
///
/// [CR 615.7]: https://yawgatog.com/resources/magic-rules/#R6157
/// [CR 615.3]: https://yawgatog.com/resources/magic-rules/#R6153
pub static CARD: CardDef = Printed::MendingHands
    .abilities(&[Ability::spell()
        .target(&TargetSpec::any_target())
        .effect(&prevent_next_damage_to_any_target(4))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const MENDING: &str = Card::MendingHands;
    const BOLT: &str = Card::LightningBolt;

    pub const SCENARIOS: &[Scenario] = &[
        // The shield prevents 3 of the first Bolt's damage and the 1 it has left of the
        // second's, so P0 takes 2 of 6 ([CR 615.7]).
        //
        // [CR 615.7]: https://yawgatog.com/resources/magic-rules/#R6157
        Scenario::new("Mending Hands prevents 4 of two Lightning Bolts' 6 damage")
            .interaction(&[BOLT])
            .battlefield(P0, &[Card::Plains, Card::Mountain, Card::Mountain])
            .hand(P0, &[MENDING, BOLT, BOLT])
            .actions(&[
                ME.casts(MENDING),
                ME.targets_player(ME),
                Resolve,
                ME.casts(BOLT),
                ME.targets_player(ME),
                Resolve,
                ME.life(20),
                ME.casts(BOLT),
                ME.targets_player(ME),
                Resolve,
                ME.life(18),
            ]),
        Scenario::new("Mending Hands saves a creature from Lightning Bolt")
            .interaction(&[BOLT])
            .battlefield(P0, &[Card::Plains, Card::Mountain, Prop::Creature])
            .hand(P0, &[MENDING, BOLT])
            .actions(&[
                ME.casts(MENDING),
                ME.targets(Prop::Creature),
                Resolve,
                ME.casts(BOLT),
                ME.targets(Prop::Creature),
                Resolve,
                ME.has(Zone::Battlefield, Prop::Creature, 1),
                ME.life(20),
            ]),
    ];
}
