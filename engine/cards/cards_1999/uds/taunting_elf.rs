// Generated from the card store by `just card header`: notes go below the end marker.
//! Taunting Elf {G}
//! Creature — Elf, 0/1
//!
//! All creatures able to block this creature do so.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::TauntingElf
    .abilities(&[Ability::rule_change(&RuleChange {
        players: Players::You,
        what: Governed::MustBlock(Objects::This),
        when: When::AsUsual,
    })])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] =
        &[
            Scenario::new("All creatures able to block Taunting Elf must block it")
                .battlefield(P0, &[Card::TauntingElf, Prop::Vanilla])
                .battlefield(P1, &[Prop::Vanilla, Card::GrizzlyBears])
                .actions(&[
                    advance_to(1, TurnStep::BeginningOfCombat),
                    ME.passes(),
                    ME.attacks_with(Card::TauntingElf),
                    ME.attacks_with(Prop::Vanilla),
                    THEM.chooses(DecisionKind::Blocker, Prop::Vanilla),
                    advance_to(1, TurnStep::DeclareBlockers),
                ]),
        ];
}
