//! Teferi, Time Raveler. {1}{W}{U} Legendary Planeswalker — Teferi. Loyalty 4.
//!
//! "Each opponent can cast spells only any time they could cast a sorcery.
//! +1: Until your next turn, you may cast sorcery spells as though they had flash.
//! −3: Return up to one target artifact, creature, or enchantment to its owner's hand. Draw a
//! card."

use beryl_card::effect::build::{TARGET, YOU, draw, return_to_hand, sequence};
use beryl_engine::card::{
    Ability, AbilityKind, Behavior, CardDef, Colors, Cost, Face, Flow, ManaCost, Rule, Subtype,
    Subtypes, Supertypes, TargetKind, TargetSpec, Types,
};
use beryl_engine::effect::EffectCtx;
use beryl_engine::filter::Filter;
use beryl_engine::state::{Affects, Duration, Payload};
use beryl_engine::{Layout, bodies};

use crate::names;

/// Planeswalkers' loyalty abilities ([CR 606.3](https://yawgatog.com/resources/magic-rules/#R6063)):
/// "A player may activate a loyalty ability of a permanent they control any time they have
/// priority and the stack is empty during a main phase of their turn, but only if no player
/// has previously activated a loyalty ability of that permanent that turn."
pub static CARD: CardDef = CardDef {
    name: names::TEFERI_TIME_RAVELER,
    faces: &[Face {
        cost: ManaCost::new(1, [1, 1, 0, 0, 0, 0]),
        supertypes: Supertypes::LEGENDARY,
        types: Types::PLANESWALKER,
        subtypes: Subtypes::of(&[Subtype::Teferi]),
        colors: Colors::WHITE.union(Colors::BLUE),
        loyalty: Some(4),
        abilities: &[
            // A prohibition the casting machine reads: a "can't" beats a "can"
            // ([CR 101.2](https://yawgatog.com/resources/magic-rules/#R1012)), so even a
            // spell with flash waits for sorcery timing.
            Ability {
                kind: AbilityKind::Static,
                rule: Rule::OpponentsCastAsSorceries,
                ..Ability::NONE
            },
            Ability {
                kind: AbilityKind::Activated,
                cost: Cost::loyalty(1),
                body: bodies!(sorceries_as_flash),
                ..Ability::NONE
            },
            // "−3: Return up to one target artifact, creature, or enchantment to its owner's
            // hand. Draw a card." With no target chosen it still draws; with its one target
            // illegal, it doesn't resolve at all
            // ([CR 608.2b](https://yawgatog.com/resources/magic-rules/#R6082b)).
            Ability {
                kind: AbilityKind::Activated,
                cost: Cost::loyalty(-3),
                targets: &[TargetSpec::up_to_one(
                    TargetKind::Permanent,
                    Filter::Types(
                        Types::ARTIFACT
                            .union(Types::CREATURE)
                            .union(Types::ENCHANTMENT),
                    ),
                )],
                body: Behavior::Data(&sequence(&[return_to_hand(TARGET), draw(YOU, 1)])),
                ..Ability::NONE
            },
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "+1: Until your next turn, you may cast sorcery spells as though they had flash."
/// [CR 702.8a](https://yawgatog.com/resources/magic-rules/#R7028a): "'Flash' means 'You may
/// play this card any time you could cast an instant.'" A change to a rule for one player
/// ([CR 613.11](https://yawgatog.com/resources/magic-rules/#R61311): "Some continuous effects
/// affect game rules rather than objects."), which ends as their next turn begins.
fn sorceries_as_flash<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    let you = ctx.controller();
    ctx.add_effect(
        Affects::Player(you),
        Payload::Rule(Rule::SorceriesAsFlash),
        Duration::UntilYourNextTurn(you),
    );
    Ok(())
}

#[cfg(feature = "scenarios")]
pub mod scenarios {

    use beryl_engine::{DecisionKind, TurnStep, Zone};

    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use crate::names;

    const TEFERI: &str = names::TEFERI_TIME_RAVELER;

    const ICEBERG: &str = names::AANGS_ICEBERG;

    const STAFF: &str = names::STAFF_OF_THE_STORYTELLER;

    const JACE: &str = names::JACE_VRYNS_PRODIGY;

    pub const SCENARIOS: &[Scenario] = &[
        // [CR 506.4](https://yawgatog.com/resources/magic-rules/#R5064): "A planeswalker or battle
        // that’s removed from combat stops being attacked."
        Scenario::new("combat damage removes Teferi while other attacks remain")
            .battlefield(
                P0,
                &[
                    Permanent::new(names::BEZA_THE_BOUNDING_SPRING),
                    Permanent::new(names::SNAPCASTER_MAGE),
                    Permanent::new(names::SUBTLETY),
                ],
            )
            .battlefield(
                P1,
                &[
                    Permanent::new(TEFERI),
                    Permanent::new(names::JACE_THE_MIND_SCULPTOR),
                ],
            )
            .library(P0, &[names::ISLAND])
            .library(P1, &[names::PLAINS])
            .actions(&[
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::BeginningOfCombat,
                },
                Action::pass(P0),
                Action::choose(P0, DecisionKind::Attacker, names::BEZA_THE_BOUNDING_SPRING),
                Action::choose(P0, DecisionKind::AttackTarget, TEFERI),
                Action::choose(P0, DecisionKind::Attacker, names::SNAPCASTER_MAGE),
                Action::choose(
                    P0,
                    DecisionKind::AttackTarget,
                    names::JACE_THE_MIND_SCULPTOR,
                ),
                Action::choose(P0, DecisionKind::Attacker, names::SUBTLETY),
                Action::answer(P0, DecisionKind::AttackTarget, Pick::DefendingPlayer),
                Action::AdvanceTo {
                    turn: 1,
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
                    card: names::JACE_THE_MIND_SCULPTOR,
                    kind: beryl_engine::state::CounterKind::Loyalty,
                    count: 1,
                }),
                Action::assert(Assertion::Life {
                    player: P1,
                    life: 17,
                }),
            ]),
        // CR 101.2 and 307.1: a flash spell in an opponent's hand waits for their own main phase
        // with an empty stack.
        Scenario::new("an opponent's flash spell waits for sorcery timing")
            .battlefield(P0, &[Permanent::new(TEFERI)])
            .hand(P1, &[ICEBERG])
            .battlefield(
                P1,
                &[
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                    Permanent::new(JACE),
                ],
            )
            .library(P0, &[names::ISLAND, names::ISLAND])
            .library(P1, &[names::PLAINS, names::ISLAND])
            .actions(&[
                Action::pass(P0),
                // Passing and Jace's loot: Aang's Iceberg has flash, but Teferi forbids it now.
                Action::assert(Assertion::Decision {
                    player: P1,
                    kind: DecisionKind::Priority,
                    candidates: 2,
                }),
                Action::AdvanceTo {
                    turn: 2,
                    step: TurnStep::Upkeep,
                },
                Action::assert(Assertion::Decision {
                    player: P1,
                    kind: DecisionKind::Priority,
                    candidates: 2,
                }),
                // Nor in its own main phase while the loot is on the stack: with only passing
                // left, the loot resolves unasked, straight to its discard.
                Action::AdvanceTo {
                    turn: 2,
                    step: TurnStep::PrecombatMain,
                },
                Action::activate(P1, JACE, 0),
                Action::choose(P1, DecisionKind::ChooseObject, names::ISLAND),
                // The stack empty in its main phase: sorcery timing.
                Action::cast(P1, ICEBERG),
                Action::answer(P1, DecisionKind::TargetCount, Pick::Number { n: 1 }),
                Action::choose(P1, DecisionKind::Target, TEFERI),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Exile,
                    card: Some(TEFERI),
                    count: 1,
                }),
            ]),
        Scenario::new("−3 returns an opposing artifact and draws a card")
            .battlefield(P0, &[Permanent::new(TEFERI)])
            .battlefield(P1, &[Permanent::new(STAFF)])
            .library(P0, &[names::ISLAND])
            .library(P1, &[names::PLAINS])
            .actions(&[
                Action::activate(P0, TEFERI, 2),
                Action::answer(P0, DecisionKind::TargetCount, Pick::Number { n: 1 }),
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Hand,
                    card: Some(STAFF),
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
                    kind: beryl_engine::state::CounterKind::Loyalty,
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
            .battlefield(P1, &[Permanent::new(JACE)])
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
                Action::Resolve,
                // Second from the top.
                Action::assert(Assertion::Library {
                    player: P1,
                    top: &[names::PLAINS, JACE, names::ISLAND],
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
