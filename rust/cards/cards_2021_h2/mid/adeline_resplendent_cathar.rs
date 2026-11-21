// Generated from the card store by `just card header`: notes go below the end marker.
//! Adeline, Resplendent Cathar {1}{W}{W}
//! Legendary Creature — Human Knight, */4
//!
//! Vigilance
//! Adeline's power is equal to the number of creatures you control.
//! Whenever you attack, for each opponent, create a 1/1 white Human creature token that's tapped
//! and attacking that player or a planeswalker they control.
// End of the generated card header.

use crate::prelude::*;

pub use crate::tokens::HUMAN_TOKEN;

pub static HUMAN: CardDef = Token::HumanToken.card();

pub static CARD: CardDef = Printed::AdelineResplendentCathar
    .keywords(Keywords::VIGILANCE)
    .abilities(&[
        Ability::static_ability(&LayerSpec {
            affected: Objects::This,
            condition: None,
            modifications: &[Modification::SetPower(&Amount::Count(&Objects::All {
                zone: Zone::Battlefield,
                filter: Filter::All(&[
                    Filter::Types(Types::CREATURE),
                    Filter::Whose(&Players::You),
                ]),
            }))],
        }),
        Ability::triggered(&Trigger::attack(Players::You)).effect(&for_each_player(
            each_opponent(),
            &create_attacking_tokens(you(), &HUMAN, 1, that_player()),
        )),
    ])
    .card()
    .creating(&[&HUMAN]);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    use super::HUMAN_TOKEN;

    const ADELINE: &str = Card::AdelineResplendentCathar;

    /// Adeline attacks beside another creature: the Human enters tapped and attacking, her power
    /// counts it, and vigilance keeps her untapped.
    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Adeline's Human arrives attacking")
            .battlefield(P0, &[ADELINE, Prop::Flier])
            .actions(&[
                ME.pt(ADELINE, 2, 4),
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(ADELINE),
                ME.done_attacking(),
                stack(1),
                Resolve,
                ME.attacking(HUMAN_TOKEN, 1),
                ME.tapped(HUMAN_TOKEN, 1),
                ME.tapped(ADELINE, 0),
                ME.pt(ADELINE, 3, 4),
                advance_to(1, TurnStep::EndOfCombat),
                // Adeline's 3 and the Human's 1.
                THEM.life(16),
            ]),
        // "Attacking that player or a planeswalker they control": its controller picks the
        // planeswalker (CR 508.4), whom the Human's damage then removes loyalty from (CR 120.3c).
        Scenario::new("Adeline's Human attacks a planeswalker")
            .battlefield(P0, &[ADELINE])
            .battlefield(P1, &[Prop::Planeswalker])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(ADELINE),
                // Adeline at the player; with no other creature, attackers are declared and the
                // trigger resolves straight to its question.
                ME.answers(DecisionKind::AttackTarget, Pick::DefendingPlayer),
                ME.chooses(DecisionKind::AttackTarget, Prop::Planeswalker),
                ME.attacking(HUMAN_TOKEN, 1),
                advance_to(1, TurnStep::EndOfCombat),
                // Adeline's 2 to the player, the Human's 1 to the planeswalker.
                THEM.life(18),
                THEM.has_loyalty(Prop::Planeswalker, 2),
            ]),
    ];
}
