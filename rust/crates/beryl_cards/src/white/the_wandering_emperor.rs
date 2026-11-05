//! The Wandering Emperor. {2}{W}{W} Legendary Planeswalker. Loyalty 3.
//!
//! "Flash
//! As long as The Wandering Emperor entered this turn, you may activate her loyalty abilities
//! any time you could cast an instant.
//! +1: Put a +1/+1 counter on up to one target creature. It gains first strike until end of
//! turn.
//! −1: Create a 2/2 white Samurai creature token with vigilance.
//! −2: Exile target tapped creature. You gain 2 life."

use beryl_engine::card::{
    Ability, AbilityKind, CardDef, Colors, Cost, Face, Flow, Keywords, ManaCost, Modification,
    Rule, Subtype, Subtypes, Supertypes, TargetKind, TargetSpec, Types,
};
use beryl_engine::effect::EffectCtx;
use beryl_engine::event::{Reason, TokenEntry};
use beryl_engine::filter::Filter;
use beryl_engine::state::{Continuous, CounterKind, Duration};
use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

use crate::names;

pub static CARD: CardDef = CardDef {
    name: names::THE_WANDERING_EMPEROR,
    faces: &[Face {
        cost: ManaCost::new(2, [2, 0, 0, 0, 0, 0]),
        supertypes: Supertypes::LEGENDARY,
        types: Types::PLANESWALKER,
        colors: Colors::WHITE,
        loyalty: Some(3),
        keywords: Keywords::FLASH,
        abilities: &[
            // The exception to CR 606.3's timing, which the casting machine reads.
            Ability {
                kind: AbilityKind::Static,
                rule: Rule::InstantLoyaltyTheTurnItEntered,
                ..Ability::NONE
            },
            Ability {
                kind: AbilityKind::Activated,
                cost: Cost::loyalty(1),
                targets: &[TargetSpec::up_to_one(
                    TargetKind::Permanent,
                    Filter::Types(Types::CREATURE),
                )],
                body: counter_and_first_strike,
                ..Ability::NONE
            },
            Ability {
                kind: AbilityKind::Activated,
                cost: Cost::loyalty(-1),
                body: samurai,
                ..Ability::NONE
            },
            Ability {
                kind: AbilityKind::Activated,
                cost: Cost::loyalty(-2),
                targets: &[TargetSpec::one(
                    TargetKind::Permanent,
                    Filter::All(&[Filter::Types(Types::CREATURE), Filter::Tapped]),
                )],
                body: exile_and_gain,
                ..Ability::NONE
            },
        ],
        ..Face::EMPTY
    }],
    tokens: &[&SAMURAI],
};

/// The Samurai token's name ([CR 111.4](https://yawgatog.com/resources/magic-rules/#R1114)):
/// "If the spell or ability doesn't specify the name of the token, its name is the same as
/// its subtype(s) plus the word 'Token.'"
pub const SAMURAI_TOKEN: &str = "Samurai Token";

/// "A 2/2 white Samurai creature token with vigilance"
pub static SAMURAI: CardDef = CardDef {
    name: SAMURAI_TOKEN,
    faces: &[Face {
        types: Types::CREATURE,
        subtypes: Subtypes::of(&[Subtype::Samurai]),
        colors: Colors::WHITE,
        power: Some(2),
        toughness: Some(2),
        keywords: Keywords::VIGILANCE,
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "+1: Put a +1/+1 counter on up to one target creature. It gains first strike until end of
/// turn." [CR 702.7a](https://yawgatog.com/resources/magic-rules/#R7027a): "First strike is
/// a static ability that modifies the rules for the combat damage step."
fn counter_and_first_strike(ctx: &mut EffectCtx) -> Flow {
    if let Some(creature) = ctx.target_object(0) {
        ctx.add_counters(creature, CounterKind::PlusOne, 1);
        ctx.add_effect(
            creature,
            Continuous::Layer(Modification::AddKeywords(Keywords::FIRST_STRIKE)),
            Duration::EndOfTurn,
        );
    }
    Ok(())
}

/// "−1: Create a 2/2 white Samurai creature token with vigilance."
/// [CR 111.2](https://yawgatog.com/resources/magic-rules/#R1112): "The player who creates a
/// token is its owner. The token enters the battlefield under that player's control."
fn samurai(ctx: &mut EffectCtx) -> Flow {
    ctx.create_tokens(&SAMURAI, 1, TokenEntry::default());
    Ok(())
}

/// "−2: Exile target tapped creature. You gain 2 life."
/// [CR 701.13a](https://yawgatog.com/resources/magic-rules/#R70113a): "To exile an object,
/// move it to the exile zone from wherever it is."
fn exile_and_gain(ctx: &mut EffectCtx) -> Flow {
    if let Some(creature) = ctx.target_object(0) {
        ctx.move_to(creature, Zone::Exile, Reason::Exile);
    }
    ctx.gain_life(ctx.controller(), 2);
    Ok(())
}

const EMPEROR: &str = names::THE_WANDERING_EMPEROR;
const JACE: &str = names::JACE_VRYNS_PRODIGY;
const PLAINS: &str = names::PLAINS;
const LANDS: &[Permanent] = &[
    Permanent::new(PLAINS),
    Permanent::new(PLAINS),
    Permanent::new(PLAINS),
    Permanent::new(PLAINS),
];

pub const SCENARIOS: &[Scenario] = &[
    // Flashed in on the opponent's turn during combat, she activates at instant speed
    // (Rule::InstantLoyaltyTheTurnItEntered); the turn after, she waits for a main phase.
    Scenario::new("flashed in during combat, −2 exiles the tapped attacker")
        .at(2, P1, TurnStep::BeginningOfCombat)
        .hand(P0, &[EMPEROR])
        .battlefield(P0, LANDS)
        .battlefield(P1, &[Permanent::new(JACE)])
        .library(P0, &[PLAINS, PLAINS])
        .library(P1, &[PLAINS, PLAINS])
        .actions(&[
            Action::pass(P1),
            Action::pass(P0),
            // Jace attacks, tapped; the attackers' controller has nothing to do.
            Action::choose(P1, DecisionKind::Attacker, JACE),
            // With nothing else to do, each resolves as soon as its controller passes.
            Action::cast(P0, EMPEROR),
            Action::activate(P0, EMPEROR, 3),
            Action::assert(Assertion::Count {
                player: P1,
                zone: Zone::Exile,
                card: Some(JACE),
                count: 1,
            }),
            Action::assert(Assertion::Life {
                player: P0,
                life: 22,
            }),
            Action::assert(Assertion::Counters {
                player: P0,
                card: EMPEROR,
                kind: CounterKind::Loyalty,
                count: 1,
            }),
            // The turn after, in her controller's upkeep: no instant-speed loyalty abilities.
            Action::AdvanceTo {
                turn: 3,
                step: TurnStep::Upkeep,
            },
            Action::assert(Assertion::Decision {
                player: P0,
                kind: DecisionKind::Priority,
                candidates: 1,
            }),
            Action::AdvanceTo {
                turn: 3,
                step: TurnStep::PrecombatMain,
            },
            // A main phase with an empty stack: the land drawn and her two affordable
            // abilities (−2 has no tapped creature to target).
            Action::assert(Assertion::Decision {
                player: P0,
                kind: DecisionKind::Priority,
                candidates: 4,
            }),
        ]),
    Scenario::new("+1 grows a creature and gives it first strike")
        .battlefield(P0, &[Permanent::new(EMPEROR)])
        .battlefield(P1, &[Permanent::new(JACE)])
        .library(P0, &[PLAINS, PLAINS])
        .library(P1, &[PLAINS, PLAINS])
        .actions(&[
            Action::activate(P0, EMPEROR, 1),
            Action::answer(P0, DecisionKind::TargetCount, Pick::Number { n: 1 }),
            Action::Resolve,
            Action::assert(Assertion::Pt {
                player: P1,
                card: JACE,
                token: false,
                power: 1,
                toughness: 3,
            }),
            Action::assert(Assertion::Counters {
                player: P0,
                card: EMPEROR,
                kind: CounterKind::Loyalty,
                count: 4,
            }),
            // The counter stays after the turn.
            Action::AdvanceTo {
                turn: 2,
                step: TurnStep::Upkeep,
            },
            Action::assert(Assertion::Pt {
                player: P1,
                card: JACE,
                token: false,
                power: 1,
                toughness: 3,
            }),
        ]),
    Scenario::new("−1 makes a 2/2 Samurai")
        .battlefield(P0, &[Permanent::new(EMPEROR)])
        .hand(P0, &[PLAINS])
        .library(P0, &[PLAINS])
        .library(P1, &[PLAINS])
        .actions(&[
            Action::activate(P0, EMPEROR, 2),
            Action::assert(Assertion::Pt {
                player: P0,
                card: SAMURAI_TOKEN,
                token: true,
                power: 2,
                toughness: 2,
            }),
            Action::assert(Assertion::Counters {
                player: P0,
                card: EMPEROR,
                kind: CounterKind::Loyalty,
                count: 2,
            }),
        ]),
];
