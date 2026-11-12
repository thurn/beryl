// Generated from the card store by `just card header`: notes go below the end marker.
//! Jace, Vryn's Prodigy {1}{U}
//! Legendary Creature — Human Wizard, 0/2
//!
//! {T}: Draw a card, then discard a card. If there are five or more cards in your graveyard, exile
//! Jace, then return him to the battlefield transformed under his owner's control.
//!
//! Jace, Telepath Unbound
//! Legendary Planeswalker — Jace, loyalty 5
//!
//! +1: Up to one target creature gets -2/-0 until your next turn.
//! −3: You may cast target instant or sorcery card from your graveyard this turn. If that spell
//! would be put into your graveyard, exile it instead.
//! −9: You get an emblem with "Whenever you cast a spell, target opponent mills five cards."
// End of the generated card header.

use beryl_card::effect::build::{
    TARGET, TARGET_PLAYER, THIS, THOSE, YOU, discard, draw, exile, get_emblem, if_then, mill,
    modify, permit_cast, return_transformed, sequence,
};
use beryl_card::{
    Ability, Amount, CardDef, CastTerms, Comparison, Condition, Cost, Duration, Effect, Face,
    Filter, Modification, Objects, Players, TargetKind, TargetSpec, Trigger, Types, Zone, card,
};

use crate::{names, printed};

/// [CR 712.2](https://yawgatog.com/resources/magic-rules/#R7122): a double-faced card "has a
/// Magic card face on each side rather than a Magic card face on one side and a Magic card
/// back on the other."
pub static CARD: CardDef = card(
    names::JACE_VRYNS_PRODIGY,
    &[
        printed::JACE_VRYNS_PRODIGY
            .abilities(&[Ability::activated(Cost::TAP).effect(&LOOT_AND_TRANSFORM)]),
        printed::JACE_TELEPATH_UNBOUND.abilities(&[
            Ability::loyalty(1)
                .targeting(&[TargetSpec::up_to_one(
                    TargetKind::Permanent,
                    Filter::Types(Types::CREATURE),
                )])
                .effect(&WEAKEN),
            Ability::loyalty(-3)
                .targeting(&[TargetSpec::one(
                    TargetKind::GraveyardCard,
                    Filter::All(&[
                        Filter::Types(Types::INSTANT.union(Types::SORCERY)),
                        Filter::Whose(&Players::You),
                    ]),
                )])
                .effect(&RECAST),
            Ability::loyalty(-9).effect(&GET_EMBLEM),
        ]),
    ],
)
.creating(&[&EMBLEM]);

/// The emblem of the −9 ([CR 114.1](https://yawgatog.com/resources/magic-rules/#R1141)): "An
/// emblem is a marker used to represent an object that has one or more abilities, but usually
/// no other characteristics."
pub static EMBLEM: CardDef = card(
    "Jace, Telepath Unbound Emblem",
    &[Face::EMPTY.abilities(&[
        Ability::triggered(&Trigger::casts(Players::You, Filter::Any))
            .effect(&MILL_FIVE)
            .targeting(&[TargetSpec::one(
                TargetKind::Player,
                Filter::Whose(&Players::EachOpponent),
            )]),
    ])],
);

/// "The cards in your graveyard".
static YOUR_GRAVEYARD: Objects = Objects::All {
    zone: Zone::Graveyard,
    filter: Filter::Whose(&Players::You),
};

/// "{T}: Draw a card, then discard a card. If there are five or more cards in your graveyard,
/// exile Jace, then return him to the battlefield transformed under his owner's control."
/// The graveyard is counted after the discard, as the instructions are followed in order
/// ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c)). A Jace that has left
/// the battlefield is not exiled, and one that was not exiled does not return
/// ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)).
static LOOT_AND_TRANSFORM: Effect = sequence(&[
    draw(YOU, 1),
    discard(YOU, 1),
    if_then(
        Condition::Compare(
            Amount::Count(&YOUR_GRAVEYARD),
            Comparison::AtLeast,
            Amount::Fixed(5),
        ),
        &sequence(&[exile(THIS), return_transformed(THOSE)]),
    ),
]);

/// "+1: Up to one target creature gets -2/-0 until your next turn." A layer 7c effect
/// ([CR 613.4c](https://yawgatog.com/resources/magic-rules/#R6134c)): "Effects and counters
/// that modify power and/or toughness (but don't set power and/or toughness to a specific
/// number or value) are applied." It lasts "as long as stated by the spell or ability creating
/// it" ([CR 611.2a](https://yawgatog.com/resources/magic-rules/#R6112a)).
static WEAKEN: Effect = modify(
    TARGET,
    Modification::ModifyPt(-2, 0),
    Duration::UntilYourNextTurn,
);

/// "−3: You may cast target instant or sorcery card from your graveyard this turn. If that
/// spell would be put into your graveyard, exile it instead." The permission carries the
/// replacement to the spell it casts: a new object on the stack
/// ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)), which "exile it instead"
/// follows wherever it would go to the graveyard: resolving, countered or fizzling
/// ([CR 614.1a](https://yawgatog.com/resources/magic-rules/#R6141a)).
static RECAST: Effect = permit_cast(
    YOU,
    TARGET,
    Duration::EndOfTurn,
    CastTerms::PLAIN.exiled_instead(),
);

/// "−9: You get an emblem with 'Whenever you cast a spell, target opponent mills five
/// cards.'" [CR 114.2](https://yawgatog.com/resources/magic-rules/#R1142): "This means that
/// [player] puts an emblem with [ability] into the command zone. The emblem is both owned and
/// controlled by that player."
static GET_EMBLEM: Effect = get_emblem(YOU, &EMBLEM);

/// "Whenever you cast a spell, target opponent mills five cards."
/// [CR 701.17a](https://yawgatog.com/resources/magic-rules/#R70117a): "For a player to mill a
/// number of cards, that player puts that many cards from the top of their library into their
/// graveyard."
static MILL_FIVE: Effect = mill(TARGET_PLAYER, 5);

/// The back face's name, which the transformed permanent has (CR 712.6d).
const TELEPATH: &str = "Jace, Telepath Unbound";

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_card::CounterKind;
    use beryl_card::{DecisionKind, TurnStep, Zone};

    use beryl_scenario_types::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use super::TELEPATH;
    use crate::{names, props};

    const JACE: &str = names::JACE_VRYNS_PRODIGY;

    const ISLAND: &str = names::ISLAND;

    const PLAINS: &str = names::PLAINS;

    const ARTIFACT: &str = props::ARTIFACT;

    const fn loyalty(count: u8) -> Action {
        Action::assert(Assertion::Counters {
            player: P0,
            card: TELEPATH,
            kind: CounterKind::Loyalty,
            count,
        })
    }

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("looting to five cards in the graveyard transforms him")
            .battlefield(P0, &[Permanent::new(JACE)])
            .hand(P0, &[PLAINS])
            .graveyard(P0, &[ISLAND, ISLAND, ISLAND])
            .library(P0, &[ISLAND, ISLAND, ISLAND])
            .library(P1, &[PLAINS, PLAINS])
            .actions(&[
                // Four in the graveyard after the first loot: no transformation.
                Action::activate(P0, JACE, 0),
                Action::Resolve,
                Action::choose(P0, DecisionKind::ChooseObject, PLAINS),
                Action::assert(Assertion::Creature {
                    player: P0,
                    card: JACE,
                    creature: true,
                }),
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::PrecombatMain,
                },
                // Nothing else to do: the loot resolves as soon as its controller passes, and
                // with only Islands in hand, the discard needs no choice.
                Action::activate(P0, JACE, 0),
                // Exiled and returned with his back face up: a planeswalker with loyalty 5.
                Action::assert(Assertion::Creature {
                    player: P0,
                    card: TELEPATH,
                    creature: false,
                }),
                loyalty(5),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Graveyard,
                    card: None,
                    count: 5,
                }),
                // A new object, so its loyalty abilities may be activated this turn. No creature
                // to target: none is chosen.
                Action::activate(P0, TELEPATH, 0),
                loyalty(6),
            ]),
        Scenario::new("+1 shrinks a creature until his controller's next turn")
            .battlefield(P0, &[Permanent::new(JACE).transformed()])
            .battlefield(P1, &[Permanent::new(JACE)])
            .library(P0, &[ISLAND, ISLAND])
            .library(P1, &[PLAINS, PLAINS])
            .actions(&[
                Action::activate(P0, TELEPATH, 0),
                Action::answer(P0, DecisionKind::TargetCount, Pick::Number { n: 1 }),
                Action::Resolve,
                loyalty(6),
                Action::assert(Assertion::Pt {
                    player: P1,
                    card: JACE,
                    token: false,
                    power: -2,
                    toughness: 2,
                }),
                Action::AdvanceTo {
                    turn: 2,
                    step: TurnStep::BeginningOfCombat,
                },
                Action::pass(P1),
                Action::answer(P1, DecisionKind::Attacker, Pick::Done),
                Action::AdvanceTo {
                    turn: 2,
                    step: TurnStep::End,
                },
                Action::assert(Assertion::Pt {
                    player: P1,
                    card: JACE,
                    token: false,
                    power: -2,
                    toughness: 2,
                }),
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::Upkeep,
                },
                Action::assert(Assertion::Pt {
                    player: P1,
                    card: JACE,
                    token: false,
                    power: 0,
                    toughness: 2,
                }),
            ]),
        Scenario::new("−3 recasts a sorcery from the graveyard, which is then exiled")
            .battlefield(
                P0,
                &[Permanent::new(JACE).transformed(), Permanent::new(PLAINS)],
            )
            .graveyard(P0, &[names::OUST])
            // A land to play keeps the game in this main phase for the assertions.
            .hand(P0, &[ISLAND])
            .battlefield(P1, &[Permanent::new(JACE)])
            .library(P0, &[ISLAND])
            .library(P1, &[PLAINS, ISLAND])
            .actions(&[
                Action::activate(P0, TELEPATH, 1),
                Action::Resolve,
                loyalty(2),
                Action::cast(P0, names::OUST),
                Action::Resolve,
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Exile,
                    card: Some(names::OUST),
                    count: 1,
                }),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Graveyard,
                    card: None,
                    count: 0,
                }),
                Action::assert(Assertion::Library {
                    player: P1,
                    top: &[PLAINS, JACE, ISLAND],
                }),
            ]),
        Scenario::new("−9's emblem mills an opponent five whenever its owner casts a spell")
            .battlefield(
                P0,
                &[
                    Permanent::new(JACE)
                        .transformed()
                        .counters(CounterKind::Loyalty, 9),
                    Permanent::new(PLAINS),
                    Permanent::new(PLAINS),
                ],
            )
            .hand(P0, &[ARTIFACT, PLAINS])
            .library(P0, &[ISLAND])
            .library(P1, &[PLAINS, PLAINS, PLAINS, PLAINS, PLAINS, ISLAND])
            .actions(&[
                Action::activate(P0, TELEPATH, 2),
                // Loyalty 0: he is put into his owner's graveyard; the emblem stays.
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Graveyard,
                    card: Some(JACE),
                    count: 1,
                }),
                // The trigger goes on the stack above the artifact and resolves first.
                Action::cast(P0, ARTIFACT),
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Graveyard,
                    card: Some(PLAINS),
                    count: 5,
                }),
                Action::assert(Assertion::Library {
                    player: P1,
                    top: &[ISLAND],
                }),
                Action::assert(Assertion::Controls {
                    player: P0,
                    card: ARTIFACT,
                    count: 1,
                }),
            ]),
    ];
}
