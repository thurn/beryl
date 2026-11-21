//! Casting spells ([CR 601](https://yawgatog.com/resources/magic-rules/#R601)) and putting
//! abilities on the stack ([CR 603](https://yawgatog.com/resources/magic-rules/#R603)).

use beryl_engine::card::{Castings, FaceIx};
use beryl_engine::state::CounterKind;
use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

use beryl_scenario_types::{ME, Resolve, THEM, advance_to, stack};

use crate::{names, props};

const MALCOLM: &str = names::MALCOLM_ALLURING_SCOUNDREL;

/// The creature attacks alone in `turn` and its trigger resolves.
macro_rules! hit {
    ($turn:expr) => {
        [
            Action::AdvanceTo {
                turn: $turn,
                step: TurnStep::BeginningOfCombat,
            },
            Action::pass(P0),
            Action::choose(P0, DecisionKind::Attacker, MALCOLM),
            Action::AdvanceTo {
                turn: $turn,
                step: TurnStep::CombatDamage,
            },
            Action::Resolve,
        ]
    };
}

const HITS: [[Action; 5]; 4] = [hit!(1), hit!(3), hit!(5), hit!(7)];

/// A card an effect lets a player cast is one they may cast
/// ([CR 608.2g](https://yawgatog.com/resources/magic-rules/#R6082g)): "If an effect
/// specifically instructs or allows a player to cast a spell during resolution, they do so
/// by following the steps in rules 601.2a-i". A land is no spell to cast
/// ([CR 305.9](https://yawgatog.com/resources/magic-rules/#R3059)), so with a land the card
/// discarded on the fourth hit nothing is offered and it stays in the graveyard.
pub const SCENARIOS: &[Scenario] =
    &[
        Scenario::new("An effect that lets a player cast a card offers no land")
            .permanents(P0, &[Permanent::new(MALCOLM)])
            .library(P0, &[names::ISLAND; 7])
            .library(P1, &[names::PLAINS, names::PLAINS, names::PLAINS])
            .actions(&[
                HITS[0][0],
                HITS[0][1],
                HITS[0][2],
                HITS[0][3],
                HITS[0][4],
                HITS[1][0],
                HITS[1][1],
                HITS[1][2],
                HITS[1][3],
                HITS[1][4],
                HITS[2][0],
                HITS[2][1],
                HITS[2][2],
                HITS[2][3],
                HITS[2][4],
                HITS[3][0],
                HITS[3][1],
                HITS[3][2],
                HITS[3][3],
                HITS[3][4],
                Action::assert(Assertion::Counters {
                    player: P0,
                    card: MALCOLM,
                    kind: CounterKind::Chorus,
                    count: 4,
                }),
                Action::assert(Assertion::Stack { count: 0 }),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Graveyard,
                    card: Some(names::ISLAND),
                    count: 4,
                }),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Hand,
                    card: Some(names::ISLAND),
                    count: 3,
                }),
            ]),
    ];

const JACE: &str = names::JACE_VRYNS_PRODIGY;

const TELEPATH: &str = "Jace, Telepath Unbound";

/// A permission to cast a card "this turn" ends with the turn
/// ([CR 601.3](https://yawgatog.com/resources/magic-rules/#R6013)): "A player can begin to
/// cast a spell only if a rule or effect allows that player to cast it and no rule or effect
/// prohibits that player from casting it." On the player's next turn the sorcery in their
/// graveyard is no card they may cast.
pub const PERMITTED: &[Scenario] =
    &[
        Scenario::new("A card a player may cast this turn is not one they may cast the next")
            .permanents(
                P0,
                &[
                    Permanent::new(JACE).transformed(),
                    Permanent::new(names::PLAINS),
                ],
            )
            .graveyard(P0, &[names::OUST])
            .hand(P0, &[names::ISLAND])
            .permanents(P1, &[Permanent::new(JACE)])
            .library(P0, &[names::ISLAND, names::ISLAND])
            .library(P1, &[names::PLAINS, names::ISLAND])
            .actions(&[
                Action::activate(P0, TELEPATH, 1),
                Action::Resolve,
                // To pass, the land or the sorcery.
                Action::assert(Assertion::Decision {
                    player: P0,
                    kind: DecisionKind::Priority,
                    candidates: 3,
                }),
                Action::AdvanceTo {
                    turn: 2,
                    step: TurnStep::BeginningOfCombat,
                },
                Action::pass(P1),
                Action::answer(P1, DecisionKind::Attacker, Pick::Done),
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::PrecombatMain,
                },
                // To pass, the land or the planeswalker's first ability: no sorcery.
                Action::assert(Assertion::Decision {
                    player: P0,
                    kind: DecisionKind::Priority,
                    candidates: 3,
                }),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Graveyard,
                    card: Some(names::OUST),
                    count: 1,
                }),
            ]),
    ];

const SAMBAR: &str = props::VANILLA.card;
const BLADE: &str = props::FLASH_CREATURE.card;

pub const SORCERY_TIMING: &[Scenario] = &[
    Scenario::new("A creature spell needs enough mana")
        .hand(P0, &[SAMBAR, names::ISLAND])
        .battlefield(P0, &[names::ISLAND])
        .actions(&[
            ME.deciding(DecisionKind::Priority, 2), // Pass or play the land.
            ME.plays(names::ISLAND),
            ME.casts(SAMBAR),
            ME.tapped(names::ISLAND, 2),
            Resolve,
            ME.controls(SAMBAR, 1),
        ]),
    Scenario::new("A creature without flash waits for the main phase and an empty stack")
        .at(3, P0, TurnStep::Upkeep)
        .hand(P0, &[SAMBAR, BLADE])
        .battlefield(
            P0,
            &[
                names::SWAMP,
                names::ISLAND,
                names::ISLAND,
                names::ISLAND,
                names::ISLAND,
            ],
        )
        .actions(&[
            ME.deciding(DecisionKind::Priority, 2), // Pass or cast the flash creature.
            advance_to(3, TurnStep::PrecombatMain),
            ME.deciding(DecisionKind::Priority, 4), // Both spells and the drawn land.
            ME.casts(BLADE),
            stack(1),
            ME.deciding(DecisionKind::Priority, 1),
            Resolve,
            ME.casts(SAMBAR),
            Resolve,
            ME.controls(SAMBAR, 1),
            ME.controls(BLADE, 1),
        ]),
    Scenario::new("A creature spell uses the stack and enters under its caster's control")
        .hand(P0, &[SAMBAR, names::PLAINS])
        .battlefield(P0, &[names::ISLAND; 2])
        .actions(&[
            ME.casts(SAMBAR),
            stack(1),
            Action::assert(Assertion::StackObject {
                depth: 0,
                name: SAMBAR,
                spell: true,
                controller: P0,
            }),
            ME.tapped(names::ISLAND, 2),
            Action::assert(Assertion::ManaPool {
                player: P0,
                amounts: [0; 6],
            }),
            ME.deciding(DecisionKind::Priority, 1),
            Resolve,
            stack(0),
            ME.controls(SAMBAR, 1),
            ME.deciding(DecisionKind::Priority, 2),
        ]),
];

pub const LEGAL_TARGETS: &[Scenario] = &[
    Scenario::new("A removal spell waits until there is a legal target")
        .hand(P0, &[names::GET_LOST, names::ISLAND])
        .battlefield(P0, &[names::PLAINS; 2])
        .hand(P1, &[SAMBAR])
        .battlefield(P1, &[names::ISLAND; 2])
        .actions(&[
            ME.deciding(DecisionKind::Priority, 2), // Pass or play the land.
            advance_to(2, TurnStep::PrecombatMain),
            THEM.casts(SAMBAR),
            Resolve,
            THEM.passes(),
            ME.casts(names::GET_LOST),
            Resolve,
            THEM.has(Zone::Graveyard, SAMBAR, 1),
        ]),
    Scenario::new("A removal spell can choose either player's creature")
        .hand(P0, &[names::GET_LOST])
        .battlefield(P0, &[names::PLAINS, names::PLAINS, SAMBAR])
        .battlefield(P1, &[props::FLIER.card])
        .actions(&[
            ME.casts(names::GET_LOST),
            ME.deciding(DecisionKind::Target, 2),
            ME.targets(props::FLIER.card),
            Resolve,
            THEM.has(Zone::Graveyard, props::FLIER.card, 1),
            ME.controls(SAMBAR, 1),
        ]),
];

pub const KICKER_X: &[Scenario] =
    &[
        Scenario::new("Kicker X offers only the positive values its caster can pay")
            .interaction(&[names::MANA_CRYPT])
            .hand(P0, &[names::THIEVING_SKYDIVER])
            .battlefield(P0, &[names::ISLAND; 4])
            .battlefield(P1, &[names::MANA_CRYPT])
            .actions(&[
                ME.deciding(DecisionKind::Priority, 3), // Pass, cast normally or kick.
                Action::Cast {
                    player: P0,
                    card: names::THIEVING_SKYDIVER,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                ME.deciding(DecisionKind::X, 2),
                ME.x(2),
                ME.tapped(names::ISLAND, 4),
                Resolve,
                Resolve,
                ME.controls(names::THIEVING_SKYDIVER, 1),
                ME.controls(names::MANA_CRYPT, 1),
            ]),
    ];

pub const OFFSPRING: &[Scenario] =
    &[
        Scenario::new("An unaffordable offspring cost is not offered")
            .hand(P0, &[names::THUNDERTRAP_TRAINER])
            .battlefield(P0, &[names::ISLAND; 2])
            .actions(&[
                ME.deciding(DecisionKind::Priority, 2),
                ME.casts(names::THUNDERTRAP_TRAINER),
                Resolve,
                Resolve,
                ME.controls(names::THUNDERTRAP_TRAINER, 1),
            ]),
    ];

pub const IMPENDING: &[Scenario] =
    &[
        Scenario::new("An impending spell is offered when its normal cost is unaffordable")
            .hand(P0, &[names::OVERLORD_OF_THE_MISTMOORS])
            .battlefield(P0, &[names::PLAINS; 4])
            .actions(&[
                ME.deciding(DecisionKind::Priority, 2),
                Action::Cast {
                    player: P0,
                    card: names::OVERLORD_OF_THE_MISTMOORS,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                Resolve,
                ME.counters(names::OVERLORD_OF_THE_MISTMOORS, CounterKind::Time, 4),
                ME.creature(names::OVERLORD_OF_THE_MISTMOORS, false),
                Resolve,
            ]),
    ];

pub const LOYALTY_TIMING: &[Scenario] =
    &[
        Scenario::new("Loyalty costs and timing limit the activations a planeswalker offers")
            .at(3, P0, TurnStep::Upkeep)
            .permanents(
                P0,
                &[Permanent::new(names::TEFERI_TIME_RAVELER).counters(CounterKind::Loyalty, 2)],
            )
            .battlefield(P1, &[props::FLIER.card])
            .actions(&[
                advance_to(3, TurnStep::Upkeep),
                ME.deciding(DecisionKind::Priority, 1),
                advance_to(3, TurnStep::PrecombatMain),
                ME.deciding(DecisionKind::Priority, 3), // Pass, play the drawn land or activate +1.
                ME.activates(names::TEFERI_TIME_RAVELER, 1),
                ME.has_loyalty(names::TEFERI_TIME_RAVELER, 3),
                Resolve,
                ME.deciding(DecisionKind::Priority, 2), // Pass or play the land, even with 3 loyalty.
            ]),
    ];

pub const SACRIFICED_SOURCE: &[Scenario] =
    &[
        Scenario::new("An activated ability resolves after its source was sacrificed")
            .battlefield(P0, &[names::STRIP_MINE])
            .battlefield(P1, &[names::ISLAND])
            .actions(&[
                ME.activates(names::STRIP_MINE, 1),
                ME.deciding(DecisionKind::Target, 2),
                ME.targets(names::ISLAND),
                ME.has(Zone::Graveyard, names::STRIP_MINE, 1),
                THEM.controls(names::ISLAND, 1),
                Resolve,
                THEM.has(Zone::Graveyard, names::ISLAND, 1),
            ]),
    ];

pub const TRIGGER_TARGETS: &[Scenario] = &[
    Scenario::new("A triggered ability chooses its target before it can resolve")
        .hand(P0, &[props::ARTIFACT.card])
        .battlefield(P0, &[names::PLAINS])
        .battlefield(P1, &[SAMBAR, props::EQUIPMENT.card])
        .actions(&[
            ME.casts(props::ARTIFACT.card),
            Resolve,
            ME.deciding(DecisionKind::Target, 2),
            ME.targets(SAMBAR),
            stack(1),
            Resolve,
            THEM.has(Zone::Exile, SAMBAR, 1),
            THEM.controls(props::EQUIPMENT.card, 1),
        ]),
    Scenario::new("A triggered ability with no legal target leaves the stack")
        .hand(P0, &[props::ARTIFACT.card])
        .battlefield(P0, &[names::PLAINS, SAMBAR])
        .actions(&[
            ME.casts(props::ARTIFACT.card),
            Resolve,
            stack(0),
            ME.controls(props::ARTIFACT.card, 1),
            ME.controls(SAMBAR, 1),
        ]),
];

const PRIOR_SPELLS: usize = 20;
const TENDRILS: &str = names::TENDRILS_OF_AGONY;
const CRYPT: &str = names::TORMODS_CRYPT;

const fn storm_hand() -> [&'static str; PRIOR_SPELLS + 1] {
    let mut hand = [CRYPT; PRIOR_SPELLS + 1];
    hand[PRIOR_SPELLS] = TENDRILS;
    hand
}

const fn storm_actions() -> [Action; 4 * PRIOR_SPELLS + 7] {
    let mut actions = [Resolve; 4 * PRIOR_SPELLS + 7];
    let cast = 2 * PRIOR_SPELLS;
    let first_copy_target = cast + 3;
    let full_stack = first_copy_target + PRIOR_SPELLS;
    let final_life = full_stack + PRIOR_SPELLS + 2;
    let mut i = 0;
    while i < PRIOR_SPELLS {
        actions[2 * i] = ME.casts(CRYPT);
        actions[first_copy_target + i] = ME.targets_player(THEM);
        i += 1;
    }
    actions[cast] = ME.casts(TENDRILS);
    actions[cast + 1] = ME.targets_player(THEM);
    actions[full_stack] = stack(21);
    actions[final_life] = ME.life(62);
    actions[final_life + 1] = THEM.life(18);
    actions
}

pub const STORM: &[Scenario] =
    &[
        Scenario::new("Storm asks for twenty targets and puts twenty copies on the stack")
            .interaction(&[CRYPT])
            .hand(P0, &storm_hand())
            .battlefield(P0, &[names::SWAMP; 4])
            .life(P1, 60)
            .actions(&storm_actions()),
    ];
