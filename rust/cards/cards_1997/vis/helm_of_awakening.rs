// Generated from the card store by `just card header`: notes go below the end marker.
//! Helm of Awakening {2}
//! Artifact
//!
//! Spells cost {1} less to cast.
// End of the generated card header.

use crate::prelude::*;

/// "Spells cost {1} less to cast." A rule change for every player, their own spells included,
/// that reduces a spell's total cost ([CR 601.2f]): "The total cost is the mana cost or
/// alternative cost (as determined in rule 601.2b), plus all additional costs and cost
/// increases, and minus all cost reductions." The reduction takes only generic mana ([CR
/// 601.2f]): "If there is no generic mana in the cost, the reduction has no effect"; it
/// cannot make the cost less than {0}. It applies after increases and before effects that set
/// the total, such as Trinisphere's minimum, and it leaves the spell's mana value as it was.
///
/// [CR 601.2f]: https://yawgatog.com/resources/magic-rules/#R6012f
pub static CARD: CardDef = Printed::HelmOfAwakening
    .abilities(&[Ability::rule_change(&RuleChange::spell_costs(
        Players::EachPlayer,
        Filter::Any,
        SpellCost::Less(1),
    ))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const HELM: &str = Card::HelmOfAwakening;
    const BOLT: &str = Card::LightningBolt;
    const THALIA: &str = Card::ThaliaGuardianOfThraben;
    const TRINISPHERE: &str = Card::Trinisphere;
    const BRAINGEYSER: &str = Card::Braingeyser;

    pub const SCENARIOS: &[Scenario] = &[
        // Braingeyser for X = 1 costs {1}{U}{U}, so it costs {U}{U}: two Islands.
        Scenario::new("Helm of Awakening: a spell costs one generic less")
            .interaction(&[BRAINGEYSER])
            .battlefield(P0, &[HELM, Card::Island, Card::Island, Card::Island])
            .hand(P0, &[BRAINGEYSER])
            .actions(&[
                ME.casts(BRAINGEYSER),
                ME.x(1),
                ME.targets_player(ME),
                ME.tapped(Card::Island, 2),
                Resolve,
                ME.has_cards(Zone::Hand, 1),
            ]),
        // The reduction never touches colored mana: Bolt still takes its Mountain.
        Scenario::new("Helm of Awakening: colored mana is not reduced")
            .interaction(&[BOLT])
            .battlefield(P0, &[HELM, Card::Mountain, Card::Mountain])
            .hand(P0, &[BOLT])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                ME.tapped(Card::Mountain, 1),
                Resolve,
                THEM.life(17),
            ]),
        // Each player's spells: on their own turn the opponent's Braingeyser for X = 1 costs
        // {U}{U} too, under P0's Helm.
        Scenario::new("Helm of Awakening: an opponent's spell costs one less")
            .interaction(&[BRAINGEYSER])
            .battlefield(P0, &[HELM])
            .battlefield(P1, &[Card::Island; 3])
            .hand(P1, &[BRAINGEYSER])
            .at(2, P1, TurnStep::PrecombatMain)
            .actions(&[
                THEM.casts(BRAINGEYSER),
                THEM.x(1),
                THEM.targets_player(THEM),
                THEM.tapped(Card::Island, 2),
            ]),
        // The ruling for Thalia: her increase and the reduction cancel, so Bolt costs {R}.
        Scenario::new("Helm of Awakening and Thalia: the increase and reduction cancel")
            .interaction(&[BOLT, THALIA])
            .battlefield(
                P0,
                &[HELM, THALIA, Card::Mountain, Card::Mountain, Card::Mountain],
            )
            .hand(P0, &[BOLT])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                ME.tapped(Card::Mountain, 1),
                Resolve,
                THEM.life(17),
            ]),
        // Reductions apply before the minimum ([CR 601.2f]): Bolt costs {R}, plus {1} from
        // Thalia, less {1} from Helm, which is {R}; Trinisphere then makes it {2}{R}. Applying
        // the minimum first would leave {1}{R}.
        Scenario::new("Helm of Awakening and Trinisphere: the reduction comes before the minimum")
            .interaction(&[BOLT, THALIA, TRINISPHERE])
            .battlefield(
                P0,
                &[
                    HELM,
                    THALIA,
                    TRINISPHERE,
                    Card::Mountain,
                    Card::Mountain,
                    Card::Mountain,
                    Card::Mountain,
                ],
            )
            .hand(P0, &[BOLT])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                ME.tapped(Card::Mountain, 3),
                Resolve,
                THEM.life(17),
            ]),
    ];
}
