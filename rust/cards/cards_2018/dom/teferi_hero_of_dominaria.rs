// Generated from the card store by `just card header`: notes go below the end marker.
//! Teferi, Hero of Dominaria {3}{W}{U}
//! Legendary Planeswalker — Teferi, loyalty 4
//!
//! +1: Draw a card. At the beginning of the next end step, untap up to two lands.
//! −3: Put target nonland permanent into its owner's library third from the top.
//! −8: You get an emblem with "Whenever you draw a card, exile target permanent an opponent
//! controls."
// End of the generated card header.

use crate::{names, printed};
use beryl_card::effect::build::{
    TARGET, YOU, delay, draw, exile, get_emblem, put_into_library, sequence, untap_up_to,
};
use beryl_card::{
    Ability, CardDef, Effect, Face, Filter, LibraryPosition, Objects, Players, TargetKind,
    TargetSpec, Trigger, TurnStep, Types, Zone, card,
};

/// The place of the delayed untap among the face's abilities.
const UNTAP: u8 = 3;

pub static CARD: CardDef = card(
    names::TEFERI_HERO_OF_DOMINARIA,
    &[printed::TEFERI_HERO_OF_DOMINARIA.abilities(&[
        Ability::loyalty(1).effect(&DRAW_AND_DELAY),
        Ability::loyalty(-3)
            .targeting(&[TargetSpec::one(
                TargetKind::Permanent,
                Filter::Not(&Filter::Types(Types::LAND)),
            )])
            .effect(&THIRD_FROM_THE_TOP),
        Ability::loyalty(-8).effect(&get_emblem(YOU, &EMBLEM)),
        Ability::triggered(&Trigger::step(TurnStep::End, Players::EachPlayer).next())
            .effect(&UNTAP_TWO_LANDS),
    ])],
)
.creating(&[&EMBLEM]);

/// The emblem of the −8 ([CR 114.1](https://yawgatog.com/resources/magic-rules/#R1141)): "An
/// emblem is a marker used to represent an object that has one or more abilities, but usually
/// no other characteristics." Teferi's controller gets it
/// ([CR 114.2](https://yawgatog.com/resources/magic-rules/#R1142)): "This means that [player]
/// puts an emblem with [ability] into the command zone. The emblem is both owned and
/// controlled by that player."
pub static EMBLEM: CardDef = card(
    "Teferi, Hero of Dominaria Emblem",
    &[
        Face::EMPTY.abilities(&[Ability::triggered(&Trigger::draws(Players::You))
            .effect(&EXILE_TARGET)
            .targeting(&[TargetSpec::one(
                TargetKind::Permanent,
                Filter::Whose(&Players::EachOpponent),
            )])]),
    ],
);

/// "+1: Draw a card. At the beginning of the next end step, untap up to two lands." The second
/// sentence creates a delayed triggered ability
/// ([CR 603.7a](https://yawgatog.com/resources/magic-rules/#R6037a)), whose source is Teferi
/// ([CR 603.7e](https://yawgatog.com/resources/magic-rules/#R6037e)): "If an activated or
/// triggered ability creates a delayed triggered ability, the source of that delayed
/// triggered ability is the same as the source of that other ability." It triggers whether or
/// not Teferi is still on the battlefield, since it refers to no object.
static DRAW_AND_DELAY: Effect = sequence(&[draw(YOU, 1), delay(UNTAP)]);

/// "Untap up to two lands": any lands, whoever controls them, chosen as it resolves.
static UNTAP_TWO_LANDS: Effect = untap_up_to(
    YOU,
    2,
    Objects::All {
        zone: Zone::Battlefield,
        filter: Filter::Types(Types::LAND),
    },
);

/// "−3: Put target nonland permanent into its owner's library third from the top." Under two
/// cards, or on the bottom of a library with fewer
/// ([CR 401.7](https://yawgatog.com/resources/magic-rules/#R4017)): "If an effect causes a
/// player to put a card into a library 'Nth from the top,' and that library has fewer than N
/// cards in it, the player puts that card on the bottom of that library." It goes to its
/// owner's library ([CR 400.3](https://yawgatog.com/resources/magic-rules/#R4003)).
static THIRD_FROM_THE_TOP: Effect = put_into_library(TARGET, LibraryPosition::FromTop(3));

/// "Whenever you draw a card, exile target permanent an opponent controls."
/// [CR 701.13a](https://yawgatog.com/resources/magic-rules/#R70113a): "To exile an object,
/// move it to the exile zone from wherever it is."
static EXILE_TARGET: Effect = exile(TARGET);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_card::CounterKind;
    use beryl_card::{DecisionKind, Seat, TurnStep, Zone};

    use beryl_scenario_types::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use crate::{names, props};

    const TEFERI: &str = names::TEFERI_HERO_OF_DOMINARIA;

    const ISLAND: &str = names::ISLAND;

    const PLAINS: &str = names::PLAINS;

    const CREATURE: &str = props::CREATURE;

    const SCULPTOR: &str = props::PLANESWALKER;

    const HOLE: &str = props::ARTIFACT;

    const fn loyalty(count: u8) -> Action {
        Action::assert(Assertion::Counters {
            player: P0,
            card: TEFERI,
            kind: CounterKind::Loyalty,
            count,
        })
    }

    const fn count(player: Seat, zone: Zone, card: Option<&'static str>, count: u8) -> Action {
        Action::assert(Assertion::Count {
            player,
            zone,
            card,
            count,
        })
    }

    const fn tapped(player: Seat, card: &'static str, count: u8) -> Action {
        Action::assert(Assertion::Tapped {
            player,
            card,
            count,
        })
    }

    const fn library(player: Seat, top: &'static [&'static str]) -> Action {
        Action::assert(Assertion::Library { player, top })
    }

    /// −3 on P1's creature, whose library is `before`; `then` checks where it went. The Island
    /// is a land and can't be chosen; Teferi himself could.
    const fn tuck(
        name: &'static str,
        before: &'static [&'static str],
        then: &'static [Action],
    ) -> Scenario {
        const TEFERIS: &[Permanent] = &[Permanent::new(TEFERI)];
        const CREATURES: &[Permanent] = &[Permanent::new(CREATURE), Permanent::new(ISLAND)];
        Scenario::new(name)
            .battlefield(P0, TEFERIS)
            .hand(P0, &[PLAINS])
            .library(P0, &[ISLAND])
            .battlefield(P1, CREATURES)
            .library(P1, before)
            .actions(then)
    }

    pub const SCENARIOS: &[Scenario] = &[
        // P0 taps both Plains for two Portable Holes, which find nothing to exile; P1's Island is
        // tapped too. The delayed ability may untap either player's lands, and P0 takes their own.
        Scenario::new("+1 draws, and at the end step untaps up to two lands")
            .battlefield(
                P0,
                &[
                    Permanent::new(TEFERI),
                    Permanent::new(PLAINS),
                    Permanent::new(PLAINS),
                ],
            )
            .hand(P0, &[HOLE, HOLE])
            .library(P0, &[ISLAND, PLAINS])
            .battlefield(P1, &[Permanent::new(ISLAND).tapped()])
            .library(P1, &[ISLAND])
            .actions(&[
                Action::cast(P0, HOLE),
                Action::Resolve,
                Action::cast(P0, HOLE),
                Action::Resolve,
                Action::activate(P0, TEFERI, 0),
                Action::Resolve,
                count(P0, Zone::Hand, Some(ISLAND), 1),
                loyalty(5),
                // Nothing untaps before the end step.
                tapped(P0, PLAINS, 2),
                tapped(P1, ISLAND, 1),
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::End,
                },
                Action::Resolve,
                Action::choose(P0, DecisionKind::ChooseUpTo, PLAINS),
                Action::choose(P0, DecisionKind::ChooseUpTo, PLAINS),
                // Read at the next decision, in turn 2, after P1's own untap step.
                tapped(P0, PLAINS, 0),
            ]),
        // The delayed ability refers to no object, so it untaps lands though Teferi is gone: P0
        // taps both Plains for two Portable Holes, and P1 returns Teferi to P0's hand with Sink
        // into Stupor. P0 untaps one Plains and stops.
        Scenario::new("+1's untap comes at the end step after Teferi has left")
            .battlefield(
                P0,
                &[
                    Permanent::new(TEFERI),
                    Permanent::new(PLAINS),
                    Permanent::new(PLAINS),
                ],
            )
            .hand(P0, &[HOLE, HOLE])
            .library(P0, &[ISLAND])
            .battlefield(
                P1,
                &[
                    Permanent::new(ISLAND),
                    Permanent::new(ISLAND),
                    Permanent::new(ISLAND),
                ],
            )
            .hand(P1, &[props::REMOVAL])
            .library(P1, &[ISLAND])
            .actions(&[
                Action::cast(P0, HOLE),
                Action::Resolve,
                Action::cast(P0, HOLE),
                Action::Resolve,
                Action::activate(P0, TEFERI, 0),
                Action::Resolve,
                Action::pass(P0),
                Action::cast(P1, props::REMOVAL),
                Action::choose(P1, DecisionKind::Target, TEFERI),
                count(P0, Zone::Hand, Some(TEFERI), 1),
                tapped(P0, PLAINS, 2),
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::End,
                },
                Action::Resolve,
                Action::choose(P0, DecisionKind::ChooseUpTo, PLAINS),
                Action::answer(P0, DecisionKind::ChooseUpTo, Pick::Done),
                tapped(P0, PLAINS, 1),
            ]),
        tuck(
            "−3 puts a nonland permanent third from the top of its owner's library",
            &[PLAINS, PLAINS, ISLAND],
            &[
                Action::activate(P0, TEFERI, 1),
                Action::choose(P0, DecisionKind::Target, CREATURE),
                Action::Resolve,
                library(P1, &[PLAINS, PLAINS, CREATURE, ISLAND]),
                count(P1, Zone::Battlefield, Some(CREATURE), 0),
                loyalty(1),
            ],
        ),
        tuck(
            "−3 into a library of fewer than two cards puts it on the bottom",
            &[PLAINS],
            &[
                Action::activate(P0, TEFERI, 1),
                Action::choose(P0, DecisionKind::Target, CREATURE),
                Action::Resolve,
                library(P1, &[PLAINS, CREATURE]),
                loyalty(1),
            ],
        ),
        // −8 from 9 leaves Teferi with 1. The emblem exiles a permanent for the draw step's card,
        // and again for the card of Teferi's own +1.
        Scenario::new("−8's emblem exiles a permanent whenever its owner draws")
            .battlefield(
                P0,
                &[Permanent::new(TEFERI).counters(CounterKind::Loyalty, 9)],
            )
            .hand(P0, &[PLAINS])
            .library(P0, &[ISLAND, ISLAND])
            .battlefield(
                P1,
                &[
                    Permanent::new(HOLE),
                    Permanent::new(HOLE),
                    Permanent::new(ISLAND),
                ],
            )
            .library(P1, &[ISLAND])
            .actions(&[
                Action::activate(P0, TEFERI, 2),
                Action::Resolve,
                loyalty(1),
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::Upkeep,
                },
                // P1 can only pass, so the draw step follows.
                Action::pass(P0),
                Action::choose(P0, DecisionKind::Target, HOLE),
                Action::Resolve,
                count(P1, Zone::Exile, Some(HOLE), 1),
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::PrecombatMain,
                },
                Action::activate(P0, TEFERI, 0),
                Action::Resolve,
                // A land is a permanent too.
                Action::choose(P0, DecisionKind::Target, ISLAND),
                Action::Resolve,
                count(P1, Zone::Exile, None, 2),
                Action::assert(Assertion::Controls {
                    player: P1,
                    card: HOLE,
                    count: 1,
                }),
            ]),
        // The Mind Sculptor's 0 draws three cards, three draws: the emblem triggers three
        // times (CR 603.2c), each trigger targeting as it goes on the stack.
        Scenario::new("−8's emblem triggers once for each card of a multiple draw")
            .battlefield(
                P0,
                &[
                    Permanent::new(TEFERI).counters(CounterKind::Loyalty, 9),
                    Permanent::new(SCULPTOR),
                ],
            )
            .library(P0, &[ISLAND, ISLAND, PLAINS, PLAINS])
            .battlefield(
                P1,
                &[
                    Permanent::new(CREATURE),
                    Permanent::new(HOLE),
                    Permanent::new(props::SMALL_CREATURE),
                    Permanent::new(ISLAND),
                ],
            )
            .library(P1, &[ISLAND])
            .actions(&[
                Action::activate(P0, TEFERI, 2),
                Action::Resolve,
                Action::activate(P0, SCULPTOR, 1),
                Action::Resolve,
                Action::choose(P0, DecisionKind::ChooseObject, ISLAND),
                Action::choose(P0, DecisionKind::ChooseObject, PLAINS),
                Action::choose(P0, DecisionKind::Target, CREATURE),
                Action::choose(P0, DecisionKind::Target, HOLE),
                Action::choose(P0, DecisionKind::Target, props::SMALL_CREATURE),
                Action::assert(Assertion::Stack { count: 3 }),
                Action::Resolve,
                Action::Resolve,
                Action::Resolve,
                count(P1, Zone::Exile, None, 3),
                Action::assert(Assertion::Controls {
                    player: P1,
                    card: ISLAND,
                    count: 1,
                }),
            ]),
        // UI regressions by-svq/by-2ml: useful colors among three lands, then undo next turn.
        Scenario::new("+1 untaps a dual land with an instant in hand")
            .battlefield(
                P0,
                &[
                    Permanent::new(TEFERI),
                    Permanent::new(PLAINS),
                    Permanent::new(ISLAND),
                    Permanent::new(names::TUNDRA),
                ],
            )
            .hand(P0, &[props::INSTANT, CREATURE])
            .library(P0, &[PLAINS, ISLAND])
            .library(P1, &[ISLAND, PLAINS])
            .actions(&[
                // Pay for a spell so these lands are tapped in every engine, after its untap step.
                // The creature, then its enters trigger, which sees an empty hand.
                Action::cast(P0, CREATURE),
                Action::Resolve,
                Action::Resolve,
                Action::activate(P0, TEFERI, 0),
                Action::Resolve,
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::End,
                },
                Action::Resolve,
                Action::choose(P0, DecisionKind::ChooseUpTo, names::TUNDRA),
                Action::choose(P0, DecisionKind::ChooseUpTo, ISLAND),
                tapped(P0, names::TUNDRA, 0),
                tapped(P0, ISLAND, 0),
            ]),
    ];
}
