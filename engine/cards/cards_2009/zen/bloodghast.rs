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
