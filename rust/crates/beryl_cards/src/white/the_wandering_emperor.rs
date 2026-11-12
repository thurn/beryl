//! The Wandering Emperor. {2}{W}{W} Legendary Planeswalker. Loyalty 3.
//!
//! "Flash
//! As long as The Wandering Emperor entered this turn, you may activate her loyalty abilities
//! any time you could cast an instant.
//! +1: Put a +1/+1 counter on up to one target creature. It gains first strike until end of
//! turn.
//! −1: Create a 2/2 white Samurai creature token with vigilance.
//! −2: Exile target tapped creature. You gain 2 life."

use crate::names;
use beryl_card::effect::build::{
    TARGET, YOU, add_counters, create_tokens, exile, gain_life, modify, sequence,
};
use beryl_card::{
    Ability, CardDef, Colors, Condition, CounterKind, Duration, Effect, Face, Filter, Governed,
    Keywords, Modification, Objects, Players, RuleChange, Subtype, Subtypes, Supertypes,
    TargetKind, TargetSpec, Types, When, card, mana,
};

/// "You may activate her loyalty abilities any time you could cast an instant."
static LOYALTY_AS_INSTANT: RuleChange = RuleChange {
    players: Players::You,
    what: Governed::ActivateLoyaltyAbilities(Objects::This),
    when: When::AsInstant,
};

/// "As long as The Wandering Emperor entered this turn".
static ENTERED_THIS_TURN: Condition = Condition::EnteredThisTurn(Objects::This);

pub static CARD: CardDef = card(
    names::THE_WANDERING_EMPEROR,
    &[Face::EMPTY
        .cost(mana("{2}{W}{W}"))
        .supertypes(Supertypes::LEGENDARY)
        .types(Types::PLANESWALKER)
        .colors(Colors::WHITE)
        .loyalty(3)
        .keywords(Keywords::FLASH)
        .abilities(&[
            Ability::rule_change(&LOYALTY_AS_INSTANT).only_if(&ENTERED_THIS_TURN),
            Ability::loyalty(1)
                .targeting(&[TargetSpec::up_to_one(
                    TargetKind::Permanent,
                    Filter::Types(Types::CREATURE),
                )])
                .effect(&COUNTER_AND_FIRST_STRIKE),
            Ability::loyalty(-1).effect(&CREATE_SAMURAI),
            Ability::loyalty(-2)
                .targeting(&[TargetSpec::one(
                    TargetKind::Permanent,
                    Filter::All(&[Filter::Types(Types::CREATURE), Filter::Tapped]),
                )])
                .effect(&EXILE_AND_GAIN),
        ])],
)
.creating(&[&SAMURAI]);

/// The Samurai token's name ([CR 111.4](https://yawgatog.com/resources/magic-rules/#R1114)):
/// "If the spell or ability doesn't specify the name of the token, its name is the same as
/// its subtype(s) plus the word 'Token.'"
pub const SAMURAI_TOKEN: &str = "Samurai Token";

/// "A 2/2 white Samurai creature token with vigilance"
pub static SAMURAI: CardDef = card(
    SAMURAI_TOKEN,
    &[Face::EMPTY
        .types(Types::CREATURE)
        .subtypes(Subtypes::of(&[Subtype::Samurai]))
        .colors(Colors::WHITE)
        .power_toughness(2, 2)
        .keywords(Keywords::VIGILANCE)],
);

/// "+1: Put a +1/+1 counter on up to one target creature. It gains first strike until end of
/// turn." [CR 702.7a](https://yawgatog.com/resources/magic-rules/#R7027a): "First strike is
/// a static ability that modifies the rules for the combat damage step."
static COUNTER_AND_FIRST_STRIKE: Effect = sequence(&[
    add_counters(TARGET, CounterKind::PlusOne, 1),
    modify(
        TARGET,
        Modification::AddKeywords(Keywords::FIRST_STRIKE),
        Duration::EndOfTurn,
    ),
]);

/// "−1: Create a 2/2 white Samurai creature token with vigilance."
/// [CR 111.2](https://yawgatog.com/resources/magic-rules/#R1112): "The player who creates a
/// token is its owner. The token enters the battlefield under that player's control."
static CREATE_SAMURAI: Effect = create_tokens(YOU, &SAMURAI, 1);

/// "−2: Exile target tapped creature. You gain 2 life."
/// [CR 701.13a](https://yawgatog.com/resources/magic-rules/#R70113a): "To exile an object,
/// move it to the exile zone from wherever it is."
static EXILE_AND_GAIN: Effect = sequence(&[exile(TARGET), gain_life(YOU, 2)]);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_engine::state::CounterKind;
    use beryl_engine::{DecisionKind, TurnStep, Zone};
    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use super::SAMURAI_TOKEN;
    use crate::{names, props};

    const EMPEROR: &str = names::THE_WANDERING_EMPEROR;

    /// A 2/1.
    const CREATURE: &str = props::SMALL_CREATURE;

    const PLAINS: &str = names::PLAINS;

    const LANDS: &[Permanent] = &[
        Permanent::new(PLAINS),
        Permanent::new(PLAINS),
        Permanent::new(PLAINS),
        Permanent::new(PLAINS),
    ];

    pub const SCENARIOS: &[Scenario] = &[
        // Flashed in on the opponent's turn during combat, she activates at instant speed
        // (her static ability's rule change); the turn after, she waits for a main phase.
        Scenario::new("flashed in during combat, −2 exiles the tapped attacker")
            .at(2, P1, TurnStep::BeginningOfCombat)
            .hand(P0, &[EMPEROR])
            .battlefield(P0, LANDS)
            .battlefield(P1, &[Permanent::new(CREATURE)])
            .library(P0, &[PLAINS, PLAINS])
            .library(P1, &[PLAINS, PLAINS])
            .actions(&[
                Action::pass(P0),
                // The creature attacks, tapped; the attackers' controller has nothing to do.
                Action::choose(P1, DecisionKind::Attacker, CREATURE),
                // With nothing else to do, each resolves as soon as its controller passes.
                Action::cast(P0, EMPEROR),
                Action::activate(P0, EMPEROR, 3),
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Exile,
                    card: Some(CREATURE),
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
            .battlefield(P1, &[Permanent::new(CREATURE)])
            .library(P0, &[PLAINS, PLAINS])
            .library(P1, &[PLAINS, PLAINS])
            .actions(&[
                Action::activate(P0, EMPEROR, 1),
                Action::answer(P0, DecisionKind::TargetCount, Pick::Number { n: 1 }),
                Action::Resolve,
                Action::assert(Assertion::Pt {
                    player: P1,
                    card: CREATURE,
                    token: false,
                    power: 3,
                    toughness: 2,
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
                    card: CREATURE,
                    token: false,
                    power: 3,
                    toughness: 2,
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
}
