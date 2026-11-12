//! Adeline, Resplendent Cathar. {1}{W}{W} Legendary Creature — Human Knight, */4.
//! "Vigilance. Adeline's power is equal to the number of creatures you control. Whenever you
//! attack, for each opponent, create a 1/1 white Human creature token that's tapped and
//! attacking that player or a planeswalker they control."

use beryl_card::Players;
use beryl_engine::card::{
    Ability, Affected, CardDef, Colors, Face, Flow, Keywords, LayerSpec, ManaCost, Modification,
    StaticCondition, Subtype, Subtypes, Supertypes, Trigger, Types,
};
use beryl_engine::effect::EffectCtx;
use beryl_engine::{Layout, bodies};

use crate::names;

/// The Human token ([CR 111.4](https://yawgatog.com/resources/magic-rules/#R1114)): "If the
/// spell or ability doesn't specify the name of the token, its name is the same as its
/// subtype(s) plus the word 'Token.'"
pub const HUMAN_TOKEN: &str = "Human Token";

/// The Human token.
pub static HUMAN: CardDef = CardDef {
    name: HUMAN_TOKEN,
    faces: &[Face {
        types: Types::CREATURE,
        subtypes: Subtypes::of(&[Subtype::Human]),
        colors: Colors::WHITE,
        power: Some(1),
        toughness: Some(1),
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "Whenever you attack" ([CR 508.3d](https://yawgatog.com/resources/magic-rules/#R5083d)):
/// it "triggers if one or more creatures that player controls are declared as attackers." One
/// Human for the one opponent, attacking what its controller chooses (CR 508.4).
fn human_attacking<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    ctx.create_attacking_tokens(&HUMAN, 1)?;
    Ok(())
}

pub static CARD: CardDef = CardDef {
    name: names::ADELINE_RESPLENDENT_CATHAR,
    faces: &[Face {
        cost: ManaCost::new(1, [2, 0, 0, 0, 0, 0]),
        supertypes: Supertypes::LEGENDARY,
        types: Types::CREATURE,
        subtypes: Subtypes::of(&[Subtype::Human, Subtype::Knight]),
        colors: Colors::WHITE,
        // The printed "*": her ability defines it. Nothing reads a creature card's power off
        // the battlefield, and layers apply on it, so 0 stands in elsewhere.
        power: Some(0),
        toughness: Some(4),
        keywords: Keywords::VIGILANCE,
        abilities: &[
            // A characteristic-defining ability
            // ([CR 604.3](https://yawgatog.com/resources/magic-rules/#R6043)), applied in
            // layer 7a ([CR 613.4a](https://yawgatog.com/resources/magic-rules/#R6134a)):
            // "Effects from characteristic-defining abilities that define power and/or
            // toughness are applied."
            Ability::static_ability(LayerSpec {
                affected: Affected::Itself,
                condition: StaticCondition::Always,
                modification: Modification::PowerIsCreaturesYouControl,
            }),
            Ability {
                may_pause: true,
                ..Ability::triggered(&Trigger::attack(Players::You), bodies!(human_attacking))
            },
        ],
        ..Face::EMPTY
    }],
    tokens: &[&HUMAN],
};

#[cfg(feature = "scenarios")]
pub mod scenarios {

    use beryl_engine::{DecisionKind, TurnStep};

    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use super::HUMAN_TOKEN;
    use crate::names;

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
        // "Attacking that player or a planeswalker they control": its controller picks Teferi
        // (CR 508.4), whom the Human's damage then removes loyalty from (CR 120.3c).
        Scenario::new("Adeline's Human attacks a planeswalker")
            .battlefield(P0, &[Permanent::new(ADELINE)])
            .battlefield(P1, &[Permanent::new(names::TEFERI_TIME_RAVELER)])
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
                Action::choose(P0, DecisionKind::AttackTarget, names::TEFERI_TIME_RAVELER),
                Action::assert(Assertion::Attacking {
                    player: P0,
                    card: HUMAN_TOKEN,
                    count: 1,
                }),
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::EndOfCombat,
                },
                // Adeline's 2 to the player, the Human's 1 to Teferi.
                Action::assert(Assertion::Life {
                    player: P1,
                    life: 18,
                }),
                Action::assert(Assertion::Counters {
                    player: P1,
                    card: names::TEFERI_TIME_RAVELER,
                    kind: beryl_engine::state::CounterKind::Loyalty,
                    count: 3,
                }),
            ]),
    ];
}
