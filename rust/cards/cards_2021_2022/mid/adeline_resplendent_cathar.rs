// Generated from the card store by `just card header`: notes go below the end marker.
//! Adeline, Resplendent Cathar {1}{W}{W}
//! Legendary Creature — Human Knight, */4
//!
//! Vigilance
//! Adeline's power is equal to the number of creatures you control.
//! Whenever you attack, for each opponent, create a 1/1 white Human creature token that's tapped
//! and attacking that player or a planeswalker they control.
// End of the generated card header.

use crate::{names, printed};
use beryl_card::effect::build::{
    EACH_OPPONENT, THAT_PLAYER, YOU, create_attacking_tokens, for_each_player,
};
use beryl_card::{
    Ability, Amount, CardDef, Filter, Keywords, LayerSpec, Modification, Objects, Players, Trigger,
    Types, Zone, card,
};

/// "The number of creatures you control".
static CREATURES_YOU_CONTROL: Amount = Amount::Count(&Objects::All {
    zone: Zone::Battlefield,
    filter: Filter::All(&[Filter::Types(Types::CREATURE), Filter::Whose(&Players::You)]),
});

/// The Human token ([CR 111.4](https://yawgatog.com/resources/magic-rules/#R1114)): "If the
/// spell or ability doesn't specify the name of the token, its name is the same as its
/// subtype(s) plus the word 'Token.'"
pub use crate::tokens::HUMAN_TOKEN;

/// The Human token.
pub static HUMAN: CardDef = card(HUMAN_TOKEN, &[printed::token::HUMAN_TOKEN]);

pub static CARD: CardDef = card(
    names::ADELINE_RESPLENDENT_CATHAR,
    &[printed::ADELINE_RESPLENDENT_CATHAR
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
                modification: Modification::SetPower(&CREATURES_YOU_CONTROL),
            }),
            // "Whenever you attack"
            // ([CR 508.3d](https://yawgatog.com/resources/magic-rules/#R5083d)): it
            // "triggers if one or more creatures that player controls are declared as
            // attackers." A Human for each opponent, attacking that player or, as its
            // controller chooses, a planeswalker they control (CR 508.4).
            Ability::triggered(&Trigger::attack(Players::You)).effect(&for_each_player(
                EACH_OPPONENT,
                &create_attacking_tokens(YOU, &HUMAN, 1, THAT_PLAYER),
            )),
        ])],
)
.creating(&[&HUMAN]);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_card::{DecisionKind, TurnStep};

    use beryl_scenario_types::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use super::HUMAN_TOKEN;
    use crate::{names, props};

    const ADELINE: &str = names::ADELINE_RESPLENDENT_CATHAR;

    /// Adeline attacks beside another creature: the Human enters tapped and attacking, her power
    /// counts it, and vigilance keeps her untapped.
    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Adeline's Human arrives attacking")
            .battlefield(
                P0,
                &[
                    Permanent::new(ADELINE),
                    Permanent::new(names::ELITE_SPELLBINDER),
                ],
            )
            .actions(&[
                Action::Assert {
                    assertion: Assertion::Pt {
                        player: P0,
                        card: ADELINE,
                        token: false,
                        power: 2,
                        toughness: 4,
                    },
                },
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::BeginningOfCombat,
                },
                Action::Answer {
                    player: P0,
                    kind: DecisionKind::Priority,
                    pick: Pick::Pass,
                },
                Action::Answer {
                    player: P0,
                    kind: DecisionKind::Attacker,
                    pick: Pick::Card { card: ADELINE },
                },
                Action::Answer {
                    player: P0,
                    kind: DecisionKind::Attacker,
                    pick: Pick::Done,
                },
                Action::Assert {
                    assertion: Assertion::Stack { count: 1 },
                },
                Action::Resolve,
                Action::Assert {
                    assertion: Assertion::Attacking {
                        player: P0,
                        card: HUMAN_TOKEN,
                        count: 1,
                    },
                },
                Action::Assert {
                    assertion: Assertion::Tapped {
                        player: P0,
                        card: HUMAN_TOKEN,
                        count: 1,
                    },
                },
                Action::Assert {
                    assertion: Assertion::Tapped {
                        player: P0,
                        card: ADELINE,
                        count: 0,
                    },
                },
                Action::Assert {
                    assertion: Assertion::Pt {
                        player: P0,
                        card: ADELINE,
                        token: false,
                        power: 3,
                        toughness: 4,
                    },
                },
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::EndOfCombat,
                },
                // Adeline's 3 and the Human's 1.
                Action::Assert {
                    assertion: Assertion::Life {
                        player: P1,
                        life: 16,
                    },
                },
            ]),
        // Adeline stays home while the Spellbinder attacks: "you attack" all the same.
        Scenario::new("Adeline's Human arrives when she stays home")
            .battlefield(
                P0,
                &[
                    Permanent::new(ADELINE),
                    Permanent::new(names::ELITE_SPELLBINDER),
                ],
            )
            .actions(&[
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::BeginningOfCombat,
                },
                Action::Answer {
                    player: P0,
                    kind: DecisionKind::Priority,
                    pick: Pick::Pass,
                },
                Action::Answer {
                    player: P0,
                    kind: DecisionKind::Attacker,
                    pick: Pick::Card {
                        card: names::ELITE_SPELLBINDER,
                    },
                },
                Action::Answer {
                    player: P0,
                    kind: DecisionKind::Attacker,
                    pick: Pick::Done,
                },
                Action::Resolve,
                Action::Assert {
                    assertion: Assertion::Attacking {
                        player: P0,
                        card: HUMAN_TOKEN,
                        count: 1,
                    },
                },
                Action::Assert {
                    assertion: Assertion::Attacking {
                        player: P0,
                        card: ADELINE,
                        count: 0,
                    },
                },
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::EndOfCombat,
                },
                // The Spellbinder's 3 and the Human's 1.
                Action::Assert {
                    assertion: Assertion::Life {
                        player: P1,
                        life: 16,
                    },
                },
            ]),
        // "Attacking that player or a planeswalker they control": its controller picks the
        // planeswalker (CR 508.4), whom the Human's damage then removes loyalty from (CR 120.3c).
        Scenario::new("Adeline's Human attacks a planeswalker")
            .battlefield(P0, &[Permanent::new(ADELINE)])
            .battlefield(P1, &[Permanent::new(props::PLANESWALKER)])
            .actions(&[
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::BeginningOfCombat,
                },
                Action::pass(P0),
                Action::choose(P0, DecisionKind::Attacker, ADELINE),
                // Adeline at the player; with no other creature, attackers are declared and the
                // trigger resolves straight to its question.
                Action::answer(P0, DecisionKind::AttackTarget, Pick::DefendingPlayer),
                Action::choose(P0, DecisionKind::AttackTarget, props::PLANESWALKER),
                Action::assert(Assertion::Attacking {
                    player: P0,
                    card: HUMAN_TOKEN,
                    count: 1,
                }),
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::EndOfCombat,
                },
                // Adeline's 2 to the player, the Human's 1 to the planeswalker.
                Action::assert(Assertion::Life {
                    player: P1,
                    life: 18,
                }),
                Action::assert(Assertion::Counters {
                    player: P1,
                    card: props::PLANESWALKER,
                    kind: beryl_card::CounterKind::Loyalty,
                    count: 2,
                }),
            ]),
    ];
}
