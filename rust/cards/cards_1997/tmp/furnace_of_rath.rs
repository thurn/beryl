// Generated from the card store by `just card header`: notes go below the end marker.
//! Furnace of Rath {1}{R}{R}{R}
//! Enchantment
//!
//! If a source would deal damage to a permanent or player, it deals double that damage to that
//! permanent or player instead.
// End of the generated card header.

use crate::prelude::*;

/// "If a source would deal damage to a permanent or player, it deals double that damage to that
/// permanent or player instead." A replacement effect ([CR 614.1a]): "Effects that use the word
/// 'instead' are replacement effects." The doubled damage still comes from its source. With a
/// prevention effect on the same damage, the player dealt the damage, or the controller of the
/// permanent dealt it, chooses which applies first ([CR 616.1]): "the affected object's
/// controller (or its owner if it has no controller) or the affected player chooses one to
/// apply".
///
/// [CR 614.1a]: https://yawgatog.com/resources/magic-rules/#R6141a
/// [CR 616.1]: https://yawgatog.com/resources/magic-rules/#R6161
pub static CARD: CardDef = Printed::FurnaceOfRath
    .abilities(&[Ability::replacing(&Replacement::multiplies_damage(2))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const FURNACE: &str = Card::FurnaceOfRath;
    const BOLT: &str = Card::LightningBolt;
    const MENDING: &str = Card::MendingHands;

    /// P0 has Furnace of Rath and casts Mending Hands on themselves, then Lightning Bolt at
    /// themselves: 3 damage meets a doubling and a shield of 4. The Plains left in hand keeps
    /// P0's main phase a decision once the Bolt has resolved.
    const fn ordering(name: &'static str) -> Scenario {
        Scenario::new(name)
            .interaction(&[BOLT, MENDING])
            .battlefield(P0, &[FURNACE, Card::Plains, Card::Mountain])
            .hand(P0, &[MENDING, BOLT, Card::Plains])
    }

    const MEND_THEN_BOLT: [Action; 6] = [
        ME.casts(MENDING),
        ME.targets_player(ME),
        Resolve,
        ME.casts(BOLT),
        ME.targets_player(ME),
        Resolve,
    ];

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Furnace of Rath doubles Lightning Bolt's damage to a player")
            .interaction(&[BOLT])
            .battlefield(P0, &[FURNACE, Card::Mountain])
            .hand(P0, &[BOLT])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                Resolve,
                THEM.life(14),
            ]),
        // "If you have two of these on the battlefield, the damage is multiplied by 4." Each
        // applies once to the event (CR 614.5).
        Scenario::new("two Furnaces of Rath multiply Lightning Bolt's damage by 4")
            .interaction(&[BOLT])
            .battlefield(P0, &[FURNACE, FURNACE, Card::Mountain])
            .hand(P0, &[BOLT])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                Resolve,
                THEM.life(8),
            ]),
        // P0's unblocked 3/1 deals 3 combat damage, doubled to 6.
        Scenario::new("Furnace of Rath doubles combat damage")
            .battlefield(P0, &[FURNACE, Prop::Flier])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(Prop::Flier),
                advance_to(1, TurnStep::EndOfCombat),
                THEM.life(14),
            ]),
        // The ruling: "That player can either (a) prevent 4 damage first and then let Furnace
        // of Rath double the remaining 1 damage, taking 2 damage, or (b) double the damage to
        // 10 and then prevent 4 damage". Here 3 damage: the shield first prevents all 3.
        ordering("preventing first, Mending Hands stops a Bolt under Furnace of Rath").actions(&[
            MEND_THEN_BOLT[0],
            MEND_THEN_BOLT[1],
            MEND_THEN_BOLT[2],
            MEND_THEN_BOLT[3],
            MEND_THEN_BOLT[4],
            MEND_THEN_BOLT[5],
            ME.picks(MENDING),
            ME.life(20),
        ]),
        // Doubling first makes 6, of which the shield prevents 4.
        ordering("doubling first, a Bolt under Furnace of Rath gets 2 past Mending Hands").actions(
            &[
                MEND_THEN_BOLT[0],
                MEND_THEN_BOLT[1],
                MEND_THEN_BOLT[2],
                MEND_THEN_BOLT[3],
                MEND_THEN_BOLT[4],
                MEND_THEN_BOLT[5],
                ME.picks(FURNACE),
                ME.life(18),
            ],
        ),
        // The doubled damage still comes from the attacker, so its lifelink gains the
        // full amount that the defending player loses.
        Scenario::new("Furnace of Rath doubles damage and life gained from Griselbrand")
            .interaction(&[Card::Griselbrand])
            .life(P0, 6)
            .battlefield(P0, &[FURNACE, Card::Griselbrand])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(Card::Griselbrand),
                ME.deciding(DecisionKind::Priority, 2),
                THEM.life(6),
                ME.life(20),
            ]),
        // The attacker assigns one damage to its blocker. The remaining five are assigned
        // to the defending player and doubled to ten.
        Scenario::new("Furnace of Rath doubles trample damage after assignment")
            .interaction(&[Card::ColossalDreadmaw])
            .battlefield(P0, &[FURNACE, Card::ColossalDreadmaw])
            .battlefield(P1, &[Prop::Vanilla])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(Card::ColossalDreadmaw),
                THEM.chooses(DecisionKind::Blocker, Prop::Vanilla),
                advance_to(1, TurnStep::EndOfCombat),
                THEM.has(Zone::Graveyard, Prop::Vanilla, 1),
                THEM.life(10),
            ]),
    ];
}
