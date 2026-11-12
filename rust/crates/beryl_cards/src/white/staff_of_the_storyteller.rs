//! Staff of the Storyteller. {1}{W} Artifact.
//!
//! "When this artifact enters, create a 1/1 white Spirit creature token with flying.
//! Whenever you create one or more creature tokens, put a story counter on this artifact.
//! {W}, {T}, Remove a story counter from this artifact: Draw a card."

use beryl_card::effect::build::{THIS, YOU, add_counters, create_tokens, draw};
use beryl_card::{Effect, Players};
use beryl_engine::card::{
    Ability, AbilityKind, Behavior, CardDef, Colors, Cost, Face, Keywords, ManaCost, Subtype,
    Subtypes, Trigger, Types,
};
use beryl_engine::filter::Filter;
use beryl_engine::state::CounterKind;

use crate::names;

pub static CARD: CardDef = CardDef {
    name: names::STAFF_OF_THE_STORYTELLER,
    faces: &[Face {
        cost: ManaCost::new(1, [1, 0, 0, 0, 0, 0]),
        types: Types::ARTIFACT,
        colors: Colors::WHITE,
        abilities: &[
            Ability::triggered(&Trigger::ENTERS, Behavior::Data(&CREATE_SPIRIT)),
            // "Whenever you create one or more creature tokens, put a story counter on this
            // artifact." One event creating several tokens triggers it once
            // ([CR 603.2c](https://yawgatog.com/resources/magic-rules/#R6032c)): "An ability
            // triggers only once each time its trigger event occurs."
            Ability::triggered(
                &Trigger::creates_tokens(Players::You, Filter::Types(Types::CREATURE)),
                Behavior::Data(&add_counters(THIS, CounterKind::Story, 1)),
            ),
            Ability {
                kind: AbilityKind::Activated,
                cost: Cost {
                    mana: ManaCost::new(0, [1, 0, 0, 0, 0, 0]),
                    tap: true,
                    remove: Some(CounterKind::Story),
                    ..Cost::NONE
                },
                body: Behavior::Data(&draw(YOU, 1)),
                ..Ability::NONE
            },
        ],
        ..Face::EMPTY
    }],
    tokens: &[&SPIRIT],
};

/// The Spirit token's name ([CR 111.4](https://yawgatog.com/resources/magic-rules/#R1114)):
/// its subtype plus the word "Token".
pub const SPIRIT_TOKEN: &str = "Spirit Token";

/// "A 1/1 white Spirit creature token with flying"
pub static SPIRIT: CardDef = CardDef {
    name: SPIRIT_TOKEN,
    faces: &[Face {
        types: Types::CREATURE,
        subtypes: Subtypes::of(&[Subtype::Spirit]),
        colors: Colors::WHITE,
        power: Some(1),
        toughness: Some(1),
        keywords: Keywords::FLYING,
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "When this artifact enters, create a 1/1 white Spirit creature token with flying."
/// [CR 111.2](https://yawgatog.com/resources/magic-rules/#R1112): "The player who creates a
/// token is its owner. The token enters the battlefield under that player's control."
static CREATE_SPIRIT: Effect = create_tokens(YOU, &SPIRIT, 1);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_engine::state::CounterKind;
    use beryl_engine::{DecisionKind, TurnStep, Zone};
    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

    use super::SPIRIT_TOKEN;
    use crate::names;

    const STAFF: &str = names::STAFF_OF_THE_STORYTELLER;

    const EMPEROR: &str = names::THE_WANDERING_EMPEROR;

    const PLAINS: &str = names::PLAINS;

    const ISLAND: &str = names::ISLAND;

    const fn story(count: u8) -> Action {
        Action::assert(Assertion::Counters {
            player: P0,
            card: STAFF,
            kind: CounterKind::Story,
            count,
        })
    }

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("its own Spirit and a Samurai each add a story counter, spent to draw")
            .interaction(&[EMPEROR])
            .hand(P0, &[STAFF])
            .battlefield(
                P0,
                &[
                    Permanent::new(PLAINS),
                    Permanent::new(PLAINS),
                    Permanent::new(PLAINS),
                    Permanent::new(EMPEROR),
                ],
            )
            .library(P0, &[ISLAND, ISLAND, ISLAND])
            .library(P1, &[PLAINS])
            .actions(&[
                Action::cast(P0, STAFF),
                Action::Resolve,
                // The enters trigger: the Spirit, and its story-counter trigger.
                Action::Resolve,
                Action::Resolve,
                story(1),
                Action::assert(Assertion::Controls {
                    player: P0,
                    card: SPIRIT_TOKEN,
                    count: 1,
                }),
                Action::activate(P0, EMPEROR, 2),
                Action::Resolve,
                Action::Resolve,
                story(2),
                Action::activate(P0, STAFF, 2),
                story(1),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Hand,
                    card: Some(ISLAND),
                    count: 1,
                }),
                Action::assert(Assertion::Tapped {
                    player: P0,
                    card: STAFF,
                    count: 1,
                }),
                // Tapped, it can't draw again this turn: passing, or playing the Island drawn.
                Action::assert(Assertion::Decision {
                    player: P0,
                    kind: DecisionKind::Priority,
                    candidates: 2,
                }),
            ]),
        // CR 603.2c: Overlord's attack makes two Insects in one event, so one story counter.
        Scenario::new("two tokens created at once add one story counter")
            .interaction(&[names::OVERLORD_OF_THE_MISTMOORS])
            .battlefield(
                P0,
                &[
                    Permanent::new(STAFF),
                    Permanent::new(names::OVERLORD_OF_THE_MISTMOORS),
                ],
            )
            .library(P0, &[ISLAND])
            .library(P1, &[PLAINS])
            .actions(&[
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::BeginningOfCombat,
                },
                Action::pass(P0),
                Action::choose(P0, DecisionKind::Attacker, names::OVERLORD_OF_THE_MISTMOORS),
                Action::assert(Assertion::Controls {
                    player: P0,
                    card: super::super::overlord_of_the_mistmoors::INSECT_TOKEN,
                    count: 2,
                }),
                story(1),
            ]),
        // UI regression by-xd4: resolving a response must not demand a pass on one's own spell.
        Scenario::new("Staff resolves after an opposing flash creature")
            .battlefield(P0, &[Permanent::new(PLAINS), Permanent::new(PLAINS)])
            .hand(P0, &[STAFF, PLAINS])
            .battlefield(P1, &[Permanent::new(ISLAND), Permanent::new(ISLAND)])
            .hand(P1, &[names::SNAPCASTER_MAGE])
            .library(P0, &[ISLAND])
            .library(P1, &[ISLAND])
            .actions(&[
                Action::cast(P0, STAFF),
                Action::cast(P1, names::SNAPCASTER_MAGE),
                story(1),
            ]),
    ];
}
