// Generated from the card store by `just card header`: notes go below the end marker.
//! Propaganda {2}{U}
//! Enchantment
//!
//! Creatures can't attack you unless their controller pays {2} for each creature they control
//! that's attacking you.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::Propaganda
    .abilities(&[Ability::rule_change(&RuleChange {
        players: Players::You,
        what: Governed::AttackCost(ManaCost::generic(2)),
        when: When::AsUsual,
    })])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        // Under Mana Reflection one Island makes {U}{U}, which pays {2} for one attacker. The
        // engine offered Wild Nacatl, but the enumerating reference for combat counted the
        // Island as one mana and called the attack unpayable.
        Scenario::new("under Mana Reflection one Island pays for an attacker")
            .interaction(&[Card::ManaReflection, Card::WildNacatl])
            .battlefield(P0, &[Card::WildNacatl, Card::ManaReflection, Card::Island])
            .battlefield(P1, &[Card::Propaganda])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                // Done and Wild Nacatl.
                ME.deciding(DecisionKind::Attacker, 2),
                ME.attacks_with(Card::WildNacatl),
                advance_to(1, TurnStep::DeclareAttackers),
                ME.tapped(Card::Island, 1),
                advance_to(1, TurnStep::PostcombatMain),
                THEM.life(19),
            ]),
    ];
}
