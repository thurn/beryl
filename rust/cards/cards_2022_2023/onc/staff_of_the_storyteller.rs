//! Staff of the Storyteller. {1}{W} Artifact.
//!
//! "When this artifact enters, create a 1/1 white Spirit creature token with flying.
//! Whenever you create one or more creature tokens, put a story counter on this artifact.
//! {W}, {T}, Remove a story counter from this artifact: Draw a card."

use crate::names;
use beryl_card::effect::build::{THIS, YOU, add_counters, create_tokens, draw};
use beryl_card::{
    Ability, CardDef, Colors, Cost, CostPart, CounterKind, Effect, Face, Filter, Keywords, Players,
    Subtype, Subtypes, Trigger, Types, card, mana,
};

pub static CARD: CardDef = card(
    names::STAFF_OF_THE_STORYTELLER,
    &[Face::EMPTY
        .cost(mana("{1}{W}"))
        .types(Types::ARTIFACT)
        .colors(Colors::WHITE)
        .abilities(&[
            Ability::triggered(&Trigger::ENTERS).effect(&CREATE_SPIRIT),
            Ability::triggered(&Trigger::creates_tokens(
                Players::You,
                Filter::Types(Types::CREATURE),
            ))
            .effect(&add_counters(THIS, CounterKind::Story, 1)),
            Ability::activated(Cost::of(
                mana("{W}"),
                &[CostPart::TAP, CostPart::remove(CounterKind::Story)],
            ))
            .effect(&draw(YOU, 1)),
        ])],
)
.creating(&[&SPIRIT]);

/// The Spirit token's name ([CR 111.4](https://yawgatog.com/resources/magic-rules/#R1114)):
/// its subtype plus the word "Token".
pub const SPIRIT_TOKEN: &str = "Spirit Token";

/// "A 1/1 white Spirit creature token with flying"
pub static SPIRIT: CardDef = card(
    SPIRIT_TOKEN,
    &[Face::EMPTY
        .types(Types::CREATURE)
        .subtypes(Subtypes::of(&[Subtype::Spirit]))
        .colors(Colors::WHITE)
        .power_toughness(1, 1)
        .keywords(Keywords::FLYING)],
);

/// "When this artifact enters, create a 1/1 white Spirit creature token with flying."
/// [CR 111.2](https://yawgatog.com/resources/magic-rules/#R1112): "The player who creates a
/// token is its owner. The token enters the battlefield under that player's control."
static CREATE_SPIRIT: Effect = create_tokens(YOU, &SPIRIT, 1);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_card::CounterKind;
    use beryl_card::{DecisionKind, TurnStep, Zone};
    use beryl_scenario_types::{Action, Assertion, P0, P1, Permanent, Scenario};

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
                    card: crate::tokens::INSECT_TOKEN,
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
