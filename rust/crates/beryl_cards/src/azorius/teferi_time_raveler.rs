//! Teferi, Time Raveler. {1}{W}{U} Legendary Planeswalker — Teferi. Loyalty 4.
//!
//! "Each opponent can cast spells only any time they could cast a sorcery.
//! +1: Until your next turn, you may cast sorcery spells as though they had flash.
//! −3: Return up to one target artifact, creature, or enchantment to its owner's hand. Draw a
//! card."

use crate::names;
use beryl_card::effect::build::{TARGET, YOU, change_rule, draw, return_to_hand, sequence};
use beryl_card::{
    Ability, CardDef, Colors, Duration, Face, Filter, Governed, Players, RuleChange, Subtype,
    Subtypes, Supertypes, TargetKind, TargetSpec, Types, When, card, mana,
};

/// "Each opponent can cast spells only any time they could cast a sorcery."
static ONLY_AS_SORCERIES: RuleChange = RuleChange {
    players: Players::EachOpponent,
    what: Governed::CastSpells(Filter::Any),
    when: When::OnlyAsSorcery,
};

/// "You may cast sorcery spells as though they had flash."
/// [CR 702.8a](https://yawgatog.com/resources/magic-rules/#R7028a): "'Flash' means 'You may
/// play this card any time you could cast an instant.'" A change to a rule for one player
/// ([CR 613.11](https://yawgatog.com/resources/magic-rules/#R61311): "Some continuous effects
/// affect game rules rather than objects.").
static SORCERIES_AS_FLASH: RuleChange = RuleChange {
    players: Players::You,
    what: Governed::CastSpells(Filter::Types(Types::SORCERY)),
    when: When::AsInstant,
};

/// Planeswalkers' loyalty abilities ([CR 606.3](https://yawgatog.com/resources/magic-rules/#R6063)):
/// "A player may activate a loyalty ability of a permanent they control any time they have
/// priority and the stack is empty during a main phase of their turn, but only if no player
/// has previously activated a loyalty ability of that permanent that turn."
pub static CARD: CardDef = card(
    names::TEFERI_TIME_RAVELER,
    &[Face::EMPTY
        .cost(mana("{1}{W}{U}"))
        .supertypes(Supertypes::LEGENDARY)
        .types(Types::PLANESWALKER)
        .subtypes(Subtypes::of(&[Subtype::Teferi]))
        .colors(Colors::WHITE.union(Colors::BLUE))
        .loyalty(4)
        .abilities(&[
            // A prohibition the casting machine reads: a "can't" beats a "can"
            // ([CR 101.2](https://yawgatog.com/resources/magic-rules/#R1012)), so even a
            // spell with flash waits for sorcery timing.
            Ability::rule_change(&ONLY_AS_SORCERIES),
            // "+1: Until your next turn, you may cast sorcery spells as though they had
            // flash." It ends as its controller's next turn begins.
            Ability::loyalty(1).effect(&change_rule(
                &SORCERIES_AS_FLASH,
                Duration::UntilYourNextTurn,
            )),
            // "−3: Return up to one target artifact, creature, or enchantment to its owner's
            // hand. Draw a card." With no target chosen it still draws; with its one target
            // illegal, it doesn't resolve at all
            // ([CR 608.2b](https://yawgatog.com/resources/magic-rules/#R6082b)).
            Ability::loyalty(-3)
                .targeting(&[TargetSpec::up_to_one(
                    TargetKind::Permanent,
                    Filter::Types(
                        Types::ARTIFACT
                            .union(Types::CREATURE)
                            .union(Types::ENCHANTMENT),
                    ),
                )])
                .effect(&sequence(&[return_to_hand(TARGET), draw(YOU, 1)])),
        ])],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_engine::{DecisionKind, TurnStep, Zone};

    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use beryl_engine::state::CounterKind;

    use crate::{names, props};

    const TEFERI: &str = names::TEFERI_TIME_RAVELER;

    pub const SCENARIOS: &[Scenario] = &[
        // [CR 506.4](https://yawgatog.com/resources/magic-rules/#R5064): "A planeswalker or battle
        // that’s removed from combat stops being attacked." Teferi's −3, with no target, leaves
        // him 1 loyalty; in P0's turn the 3/1 removes him, and the 2/1's attack on the player
        // goes on. The other planeswalker is a destination no one picks.
        Scenario::new("combat damage removes Teferi while other attacks remain")
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(
                P0,
                &[
                    Permanent::new(props::CREATURE),
                    Permanent::new(props::SMALL_CREATURE),
                ],
            )
            .battlefield(
                P1,
                &[Permanent::new(TEFERI), Permanent::new(props::PLANESWALKER)],
            )
            .library(P0, &[names::ISLAND])
            .library(P1, &[names::PLAINS, names::ISLAND])
            .actions(&[
                Action::activate(P1, TEFERI, 2),
                Action::answer(P1, DecisionKind::TargetCount, Pick::Number { n: 0 }),
                Action::assert(Assertion::Counters {
                    player: P1,
                    card: TEFERI,
                    kind: CounterKind::Loyalty,
                    count: 1,
                }),
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::BeginningOfCombat,
                },
                Action::pass(P0),
                Action::choose(P0, DecisionKind::Attacker, props::CREATURE),
                Action::choose(P0, DecisionKind::AttackTarget, TEFERI),
                Action::choose(P0, DecisionKind::Attacker, props::SMALL_CREATURE),
                Action::answer(P0, DecisionKind::AttackTarget, Pick::DefendingPlayer),
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::EndOfCombat,
                },
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Graveyard,
                    card: Some(TEFERI),
                    count: 1,
                }),
                Action::assert(Assertion::Counters {
                    player: P1,
                    card: props::PLANESWALKER,
                    kind: CounterKind::Loyalty,
                    count: 3,
                }),
                Action::assert(Assertion::Life {
                    player: P1,
                    life: 18,
                }),
            ]),
        // CR 506.4 again, with the 2/1 the last attacker declared and sent at the other
        // planeswalker: its attack goes on after Teferi leaves, and its line begins as it is
        // declared (by-w8gv.21.57).
        Scenario::new("combat damage removes Teferi while the other planeswalker is attacked")
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(
                P0,
                &[
                    Permanent::new(props::CREATURE),
                    Permanent::new(props::SMALL_CREATURE),
                ],
            )
            .battlefield(
                P1,
                &[Permanent::new(TEFERI), Permanent::new(props::PLANESWALKER)],
            )
            .library(P0, &[names::ISLAND])
            .library(P1, &[names::PLAINS, names::ISLAND])
            .actions(&[
                Action::activate(P1, TEFERI, 2),
                Action::answer(P1, DecisionKind::TargetCount, Pick::Number { n: 0 }),
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::BeginningOfCombat,
                },
                Action::pass(P0),
                Action::choose(P0, DecisionKind::Attacker, props::CREATURE),
                Action::choose(P0, DecisionKind::AttackTarget, TEFERI),
                Action::choose(P0, DecisionKind::Attacker, props::SMALL_CREATURE),
                Action::choose(P0, DecisionKind::AttackTarget, props::PLANESWALKER),
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::EndOfCombat,
                },
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Graveyard,
                    card: Some(TEFERI),
                    count: 1,
                }),
                Action::assert(Assertion::Counters {
                    player: P1,
                    card: props::PLANESWALKER,
                    kind: CounterKind::Loyalty,
                    count: 1,
                }),
                Action::assert(Assertion::Life {
                    player: P1,
                    life: 20,
                }),
            ]),
        // CR 101.2 and 307.1: a flash spell in an opponent's hand waits for their own main phase
        // with an empty stack.
        Scenario::new("an opponent's flash spell waits for sorcery timing")
            .battlefield(P0, &[Permanent::new(TEFERI)])
            .hand(P1, &[props::SMALL_CREATURE])
            .battlefield(
                P1,
                &[Permanent::new(names::ISLAND), Permanent::new(names::ISLAND)],
            )
            .library(P0, &[names::ISLAND, names::ISLAND])
            .library(P1, &[names::PLAINS, names::ISLAND])
            .actions(&[
                // Only passing in P1's upkeep: the creature has flash, but Teferi forbids it now.
                Action::AdvanceTo {
                    turn: 2,
                    step: TurnStep::Upkeep,
                },
                Action::assert(Assertion::Decision {
                    player: P1,
                    kind: DecisionKind::Priority,
                    candidates: 1,
                }),
                // The stack empty in its main phase: sorcery timing.
                Action::AdvanceTo {
                    turn: 2,
                    step: TurnStep::PrecombatMain,
                },
                Action::cast(P1, props::SMALL_CREATURE),
                Action::Resolve,
                Action::assert(Assertion::Controls {
                    player: P1,
                    card: props::SMALL_CREATURE,
                    count: 1,
                }),
            ]),
        Scenario::new("−3 returns an opposing artifact and draws a card")
            .battlefield(P0, &[Permanent::new(TEFERI)])
            .battlefield(P1, &[Permanent::new(props::ARTIFACT)])
            .library(P0, &[names::ISLAND])
            .library(P1, &[names::PLAINS])
            .actions(&[
                Action::activate(P0, TEFERI, 2),
                Action::answer(P0, DecisionKind::TargetCount, Pick::Number { n: 1 }),
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Hand,
                    card: Some(props::ARTIFACT),
                    count: 1,
                }),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Hand,
                    card: Some(names::ISLAND),
                    count: 1,
                }),
                Action::assert(Assertion::Counters {
                    player: P0,
                    card: TEFERI,
                    kind: CounterKind::Loyalty,
                    count: 1,
                }),
                // CR 606.3: one loyalty ability a turn.
                Action::assert(Assertion::Decision {
                    player: P0,
                    kind: DecisionKind::Priority,
                    candidates: 2,
                }),
            ]),
        Scenario::new("+1 lets a sorcery be cast on the opponent's turn, until the next")
            .battlefield(P0, &[Permanent::new(TEFERI), Permanent::new(names::PLAINS)])
            .hand(P0, &[names::OUST, names::OUST])
            .battlefield(P1, &[Permanent::new(props::CREATURE)])
            .library(P0, &[names::ISLAND, names::ISLAND])
            .library(P1, &[names::PLAINS, names::ISLAND])
            .actions(&[
                Action::activate(P0, TEFERI, 1),
                Action::Resolve,
                Action::AdvanceTo {
                    turn: 2,
                    step: TurnStep::Upkeep,
                },
                Action::pass(P1),
                // In the opponent's upkeep, as though it had flash (CR 702.8a).
                Action::cast(P0, names::OUST),
                // Second from the top: P1 has drawn the Plains above it since, and stops in their
                // main phase with it to play.
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Hand,
                    card: Some(names::PLAINS),
                    count: 1,
                }),
                Action::assert(Assertion::Library {
                    player: P1,
                    top: &[props::CREATURE, names::ISLAND],
                }),
                Action::assert(Assertion::Life {
                    player: P1,
                    life: 23,
                }),
                // "Until your next turn": in its controller's next upkeep the other Oust waits for
                // a main phase.
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::Upkeep,
                },
                Action::assert(Assertion::Decision {
                    player: P0,
                    kind: DecisionKind::Priority,
                    candidates: 1,
                }),
            ]),
    ];
}
