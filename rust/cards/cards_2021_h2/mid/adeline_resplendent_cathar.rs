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

/// The Human token ([CR 111.4](https://yawgatog.com/resources/magic-rules/#R1114)): "If the
/// spell or ability doesn't specify the name of the token, its name is the same as its
/// subtype(s) plus the word 'Token.'"
pub use crate::tokens::HUMAN_TOKEN;

/// The Human token.
pub static HUMAN: CardDef = Token::HumanToken.card();

/// Adeline's power is "the number of creatures you control".
pub static CARD: CardDef = Printed::AdelineResplendentCathar
    .keywords(Keywords::VIGILANCE)
    .abilities(&[
        // A characteristic-defining ability
        // ([CR 604.3](https://yawgatog.com/resources/magic-rules/#R6043)), applied in
        // layer 7a ([CR 613.4a](https://yawgatog.com/resources/magic-rules/#R6134a)):
        // "Effects from characteristic-defining abilities that define power and/or
        // toughness are applied."
        Ability::static_ability(&LayerSpec {
            affected: Objects::This,
            condition: None,
            modification: Modification::SetPower(&Amount::Count(&Objects::All {
                zone: Zone::Battlefield,
                filter: Filter::All(&[Filter::Types(Types::CREATURE), Filter::Whose(&Players::You)]),
            })),
        }),
        // "Whenever you attack"
        // ([CR 508.3d](https://yawgatog.com/resources/magic-rules/#R5083d)): it
        // "triggers if one or more creatures that player controls are declared as
        // attackers." A Human for each opponent, attacking that player or, as its
        // controller chooses, a planeswalker they control (CR 508.4).
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
            .battlefield(P0, &[ADELINE, Card::EliteSpellbinder])
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
        // Adeline stays home while the Spellbinder attacks: "you attack" all the same.
        Scenario::new("Adeline's Human arrives when she stays home")
            .battlefield(P0, &[ADELINE, Card::EliteSpellbinder])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(Card::EliteSpellbinder),
                ME.done_attacking(),
                Resolve,
                ME.attacking(HUMAN_TOKEN, 1),
                ME.attacking(ADELINE, 0),
                advance_to(1, TurnStep::EndOfCombat),
                // The Spellbinder's 3 and the Human's 1.
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
