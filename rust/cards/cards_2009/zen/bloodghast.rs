// Generated from the card store by `just card header`: notes go below the end marker.
//! Bloodghast {B}{B}
//! Creature — Vampire Spirit, 2/1
//!
//! This creature can't block.
//! This creature has haste as long as an opponent has 10 or less life.
//! Landfall — Whenever a land you control enters, you may return this card from your graveyard to
//! the battlefield.
// End of the generated card header.

use crate::prelude::*;

/// "This creature can't block." is a prohibition on declaring it as a blocker ([CR 509.1a]):
/// "The defending player chooses which creatures they control, if any, will block."
///
/// "This creature has haste as long as an opponent has 10 or less life." is a static
/// ability that grants a keyword in layer 6 ([CR 613.1f]) while its condition holds. The
/// condition reads the opponents' life ([CR 119.1]); in a two-player game that is the one
/// opponent's life total.
///
/// "Landfall — Whenever a land you control enters, you may return this card from your
/// graveyard to the battlefield." works only while the card is in a graveyard
/// ([CR 113.6b]). It triggers on a land entering under its controller's control
/// ([CR 603.2]); a land already on the battlefield that becomes a land doesn't enter. The
/// return is a "may" made on resolution ([CR 603.5]).
///
/// [CR 113.6b]: https://yawgatog.com/resources/magic-rules/#R1136b
/// [CR 119.1]: https://yawgatog.com/resources/magic-rules/#R1191
/// [CR 509.1a]: https://yawgatog.com/resources/magic-rules/#R5091a
/// [CR 603.2]: https://yawgatog.com/resources/magic-rules/#R6032
/// [CR 603.5]: https://yawgatog.com/resources/magic-rules/#R6035
/// [CR 613.1f]: https://yawgatog.com/resources/magic-rules/#R6131f
pub static CARD: CardDef = Printed::Bloodghast
    .abilities(&[
        Ability::rule_change(&RuleChange {
            players: Players::You,
            what: Governed::Block(Objects::This),
            when: When::Never,
        }),
        Ability::static_ability(&LayerSpec {
            affected: Objects::This,
            condition: Some(Condition::Compare(
                Amount::Life(Players::EachOpponent),
                Comparison::AtMost,
                Amount::Fixed(10),
            )),
            modifications: &[Modification::AddKeywords(Keywords::HASTE)],
        }),
        Ability::triggered(&Trigger::of(
            TriggerEvent::Enters,
            beryl_card::Subject::A(Filter::All(&[
                Filter::Types(Types::LAND),
                Filter::Whose(&Players::You),
            ])),
        ))
        .in_graveyard()
        .effect(&may(you(), &return_to_battlefield(this()))),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const GHAST: &str = Card::Bloodghast;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("a land entering returns it from the graveyard")
            .graveyard(P0, &[GHAST])
            .hand(P0, &[Card::Swamp])
            .actions(&[
                ME.plays(Card::Swamp),
                Resolve,
                ME.yes(),
                ME.controls(GHAST, 1),
                ME.has(Zone::Graveyard, GHAST, 0),
            ]),
        Scenario::new("declining leaves it in the graveyard")
            .graveyard(P0, &[GHAST])
            .hand(P0, &[Card::Swamp])
            .actions(&[
                ME.plays(Card::Swamp),
                Resolve,
                ME.no(),
                ME.controls(GHAST, 0),
                ME.has(Zone::Graveyard, GHAST, 1),
            ]),
        Scenario::new("a land entering while it is on the battlefield does nothing")
            .battlefield(P0, &[GHAST])
            .hand(P0, &[Card::Swamp])
            .actions(&[ME.plays(Card::Swamp), ME.controls(GHAST, 1), stack(0)]),
        Scenario::new("a land entering while it is in hand does nothing")
            .hand(P0, &[GHAST, Card::Swamp])
            .actions(&[
                ME.plays(Card::Swamp),
                ME.has(Zone::Hand, GHAST, 1),
                stack(0),
            ]),
        // Bloodghast is the only creature P0 has and it can't block, so P0 is never asked
        // to block and the 2/1 attacker deals its damage.
        Scenario::new("it can't block")
            .battlefield(P0, &[GHAST])
            .battlefield(P1, &[Prop::Vanilla])
            .library(P0, &[Card::Island, Card::Island])
            .library(P1, &[Card::Island, Card::Island])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.done_attacking(),
                advance_to(2, TurnStep::BeginningOfCombat),
                THEM.passes(),
                THEM.attacks_with(Prop::Vanilla),
                advance_to(2, TurnStep::EndOfCombat),
                ME.life(18),
            ]),
        Scenario::new("it attacks the turn it returns while an opponent has 10 or less life")
            .graveyard(P0, &[GHAST])
            .hand(P0, &[Card::Swamp])
            .life(P1, 10)
            .actions(&[
                ME.plays(Card::Swamp),
                Resolve,
                ME.yes(),
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(GHAST),
                THEM.life(8),
            ]),
    ];
}
