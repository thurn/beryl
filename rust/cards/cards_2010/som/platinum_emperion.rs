// Generated from the card store by `just card header`: notes go below the end marker.
//! Platinum Emperion {8}
//! Artifact Creature — Golem, 8/8
//!
//! Your life total can't change. (You can't gain or lose life. You can't pay any amount of life
//! except 0.)
// End of the generated card header.

use crate::prelude::*;

/// "Your life total can't change" is a rule change on its controller ([CR 613.11]: "Some continuous
/// effects affect game rules rather than objects."). They can't gain life ([CR 119.7]) or lose
/// life ([CR 119.8]): "If an effect says that a player can't lose life, that player can't make an
/// exchange such that the player's life total would become lower ... In addition, a cost that
/// involves having that player pay life can't be paid." Damage is still dealt to them, so it still
/// has its other results, such as lifelink's gain.
///
/// [CR 613.11]: https://yawgatog.com/resources/magic-rules/#R61311
/// [CR 119.7]: https://yawgatog.com/resources/magic-rules/#R1197
/// [CR 119.8]: https://yawgatog.com/resources/magic-rules/#R1198
pub static CARD: CardDef = Printed::PlatinumEmperion
    .abilities(&[Ability::rule_change(&RuleChange {
        players: Players::You,
        what: Governed::Player(PlayerRules::LIFE_CANT_CHANGE),
        when: When::AsUsual,
    })])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const EMPERION: &str = Card::PlatinumEmperion;
    const GRISELBRAND: &str = Card::Griselbrand;
    const BOLT: &str = Card::LightningBolt;

    pub const SCENARIOS: &[Scenario] = &[
        // The ruling: "You can't pay a cost that includes the payment of any amount of life other
        // than 0 life." Griselbrand's "Pay 7 life" is not offered at 20 life, so the game gives no
        // priority and goes on to declare attackers.
        Scenario::new("Griselbrand's controller can't pay 7 life")
            .interaction(&[GRISELBRAND])
            .battlefield(P0, &[EMPERION, GRISELBRAND])
            .library(P0, &[Card::Island; 8])
            .actions(&[
                ME.deciding(DecisionKind::Attacker, 3),
                ME.life(20),
                ME.has_cards(Zone::Hand, 0),
            ]),
        // The ruling: "Platinum Emperion's ability doesn't prevent damage. Rather, it changes the
        // results of that damage. For example, if a creature with lifelink deals damage to you,
        // you won't lose any life, but its controller will still gain that much life."
        Scenario::new("lifelink damage to its controller still gains life")
            .interaction(&[GRISELBRAND])
            .battlefield(P0, &[GRISELBRAND])
            // Below 7 life: Griselbrand's ability is not offered, so no priority interrupts
            // combat.
            .life(P0, 6)
            .permanents(P1, &[Permanent::new(EMPERION).tapped()])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(GRISELBRAND),
                ME.deciding(DecisionKind::Priority, 2),
                THEM.life(20),
                ME.life(13),
            ]),
        // The ruling: "Spells and abilities that would normally cause you to gain or lose life
        // still resolve, but the life-gain or life-loss part simply has no effect." Lightning
        // Bolt's 3 damage is dealt and P1 loses no life.
        Scenario::new("Lightning Bolt to its controller changes no life total")
            .interaction(&[BOLT])
            .battlefield(P0, &[Card::Mountain])
            .hand(P0, &[BOLT])
            .battlefield(P1, &[EMPERION])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                Resolve,
                THEM.life(20),
                ME.has(Zone::Graveyard, BOLT, 1),
            ]),
        // The same ruling for a spell's cost: Force of Will's alternative cost pays 1 life, so it
        // is not offered. With Lightning Bolt on the stack, P0 may pass or cast Counterspell from
        // its two Islands, and nothing else.
        Scenario::new("Force of Will's alternative cost is not offered")
            .interaction(&[BOLT, Card::ForceOfWill, Card::Counterspell])
            .battlefield(P0, &[EMPERION, Card::Island, Card::Island])
            .hand(P0, &[Card::ForceOfWill, Card::Counterspell])
            .battlefield(P1, &[Card::Mountain])
            .hand(P1, &[BOLT])
            .at(1, P1, TurnStep::PrecombatMain)
            .actions(&[
                THEM.casts(BOLT),
                THEM.targets_player(ME),
                ME.deciding(DecisionKind::Priority, 2),
                ME.casts(Card::Counterspell),
                stack(0),
                ME.has(Zone::Hand, Card::ForceOfWill, 1),
                THEM.has(Zone::Graveyard, BOLT, 1),
            ]),
    ];
}
