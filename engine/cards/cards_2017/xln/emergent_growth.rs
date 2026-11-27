// Generated from the card store by `just card header`: notes go below the end marker.
//! Emergent Growth {3}{G}
//! Sorcery
//!
//! Target creature gets +5/+5 until end of turn and must be blocked this turn if able.
// End of the generated card header.

use crate::prelude::*;

static MUST_BE_BLOCKED: RuleChange = RuleChange {
    players: Players::You,
    what: Governed::MustBeBlocked(target()),
    when: When::AsUsual,
};

pub static CARD: CardDef = Printed::EmergentGrowth
    .abilities(&[Ability::spell()
        .target(&TargetSpec::creature())
        .effect(&sequence(&[
            modify(
                target(),
                &[Modification::ModifyPt(5, 5)],
                Duration::EndOfTurn,
            ),
            change_rule(&MUST_BE_BLOCKED, Duration::EndOfTurn),
        ]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] =
        &[
            Scenario::new("Emergent Growth forces a block and its +5/+5 ends with the turn")
                .at(1, P0, TurnStep::PrecombatMain)
                .hand(P0, &[Card::EmergentGrowth])
                .battlefield(
                    P0,
                    &[
                        Card::Forest,
                        Card::Forest,
                        Card::Forest,
                        Card::Forest,
                        Prop::Vanilla,
                    ],
                )
                .battlefield(P1, &[Prop::Vanilla])
                .library(P0, &[Card::Forest, Card::Forest])
                .library(P1, &[Card::Forest, Card::Forest])
                .actions(&[
                    ME.casts(Card::EmergentGrowth),
                    ME.targets(Prop::Vanilla),
                    Resolve,
                    ME.pt(Prop::Vanilla, 7, 6),
                    advance_to(1, TurnStep::BeginningOfCombat),
                    ME.passes(),
                    ME.attacks_with(Prop::Vanilla),
                    advance_to(1, TurnStep::PostcombatMain),
                    THEM.has(Zone::Graveyard, Prop::Vanilla, 1),
                    THEM.life(20),
                    advance_to(2, TurnStep::PrecombatMain),
                    ME.pt(Prop::Vanilla, 2, 1),
                    advance_to(3, TurnStep::PrecombatMain),
                    ME.pt(Prop::Vanilla, 2, 1),
                ]),
        ];
}
