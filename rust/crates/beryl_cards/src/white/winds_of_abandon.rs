//! Winds of Abandon. {1}{W} Sorcery. "Exile target creature you don't control. For each
//! creature exiled this way, its controller searches their library for a basic land card.
//! Those players put those cards onto the battlefield tapped, then shuffle. Overload
//! {4}{W}{W} (You may cast this spell for its overload cost. If you do, change "target" in its
//! text to "each.")"

use crate::names;
use beryl_card::effect::build::{
    EACH_PLAYER, TARGET, THAT_PLAYER, exile, for_each_player, sequence,
};
use beryl_card::{
    Ability, Amount, CardDef, CastingOption, Colors, Effect, Face, Filter, Found, ManaCost,
    Objects, Players, Supertypes, TargetKind, TargetSpec, Types, Zone, card, mana,
};

/// "Creature you don't control". Not "an opponent controls": a teammate's creature is one
/// you don't control too
/// ([CR 102.3](https://yawgatog.com/resources/magic-rules/#R1023)): "the player's opponents
/// are all players not on their team."
const CREATURES_YOU_DONT_CONTROL: Filter = Filter::All(&[
    Filter::Types(Types::CREATURE),
    Filter::Not(&Filter::Whose(&Players::You)),
]);

const BASIC_LANDS: Filter = Filter::All(&[
    Filter::Types(Types::LAND),
    Filter::Supertypes(Supertypes::BASIC),
]);

/// "For each creature exiled this way, its controller searches their library for a basic
/// land card. Those players put those cards onto the battlefield tapped, then shuffle": each
/// player whose creatures were exiled searches for that many basic land cards
/// ([CR 701.23a](https://yawgatog.com/resources/magic-rules/#R70123a)) and puts them onto the
/// battlefield tapped, then shuffles
/// ([CR 701.24a](https://yawgatog.com/resources/magic-rules/#R70124a)), in turn order
/// ([CR 101.4](https://yawgatog.com/resources/magic-rules/#R1014)). A player may find fewer,
/// or none from a library without basics
/// ([CR 701.23b](https://yawgatog.com/resources/magic-rules/#R70123b)): "that player isn't
/// required to find some or all of those cards". Who searches is each creature's controller
/// as it last existed on the battlefield: in exile it has none. A creature a replacement
/// kept from exile was not "exiled this way".
const FETCH: Effect = for_each_player(
    EACH_PLAYER,
    &Effect::Search {
        player: THAT_PLAYER,
        filter: BASIC_LANDS,
        count: Amount::ThoseOf(THAT_PLAYER),
        found: Found::OntoBattlefieldTapped,
    },
);

/// "Exile target creature you don't control"
/// ([CR 701.13a](https://yawgatog.com/resources/magic-rules/#R70113a)): "To exile an object,
/// move it to the exile zone from wherever it is."
static EXILE_TARGET: Effect = sequence(&[exile(TARGET), FETCH]);

/// Overloaded, "Exile each creature you don't control"
/// ([CR 702.96a](https://yawgatog.com/resources/magic-rules/#R70296a)), with no target
/// ([CR 702.96b](https://yawgatog.com/resources/magic-rules/#R70296b)). The creatures are
/// exiled at once.
static EXILE_EACH: Effect = sequence(&[
    exile(Objects::All {
        zone: Zone::Battlefield,
        filter: CREATURES_YOU_DONT_CONTROL,
    }),
    FETCH,
]);

pub static CARD: CardDef = card(
    names::WINDS_OF_ABANDON,
    &[Face::EMPTY
        .cost(mana("{1}{W}"))
        .types(Types::SORCERY)
        .colors(Colors::WHITE)
        .abilities(&[
            Ability::spell()
                .targeting(&[TargetSpec::one(
                    TargetKind::Permanent,
                    CREATURES_YOU_DONT_CONTROL,
                )])
                .effect(&EXILE_TARGET),
            Ability::spell().effect(&EXILE_EACH),
        ])
        .casting(&[CastingOption::overload(
            ManaCost::new(4, [2, 0, 0, 0, 0, 0]),
            1,
        )])],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_engine::card::{Castings, FaceIx};

    use beryl_engine::{DecisionKind, Seat, TurnStep, Zone};

    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use crate::{names, props};

    const WINDS: &str = names::WINDS_OF_ABANDON;

    const SPELLBINDER: &str = names::ELITE_SPELLBINDER;

    /// P0's creature, or P1's second.
    const TWO_DROP: &str = props::SMALL_CREATURE;

    const fn controls(player: Seat, card: &'static str, count: u8) -> Action {
        Action::assert(Assertion::Controls {
            player,
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

    const SIX_PLAINS_AND_ADELINE: &[Permanent] = &[
        Permanent::new(names::PLAINS),
        Permanent::new(names::PLAINS),
        Permanent::new(names::PLAINS),
        Permanent::new(names::PLAINS),
        Permanent::new(names::PLAINS),
        Permanent::new(names::PLAINS),
        Permanent::new(TWO_DROP),
    ];

    pub const SCENARIOS: &[Scenario] = &[
        // Its one target is the creature P0 doesn't control; P1 finds a Plains, which enters
        // tapped, from a library with a nonbasic land too.
        Scenario::new("Winds of Abandon exiles a creature; its controller fetches a tapped basic")
            .battlefield(
                P0,
                &[
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                    Permanent::new(TWO_DROP),
                ],
            )
            .hand(P0, &[WINDS, names::PLAINS])
            .battlefield(P1, &[Permanent::new(SPELLBINDER)])
            .library(P1, &[names::TUNDRA, names::PLAINS, names::ISLAND])
            .actions(&[
                Action::cast(P0, WINDS),
                Action::Resolve,
                Action::choose(P1, DecisionKind::ChooseUpTo, names::PLAINS),
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Exile,
                    card: Some(SPELLBINDER),
                    count: 1,
                }),
                controls(P0, TWO_DROP, 1),
                controls(P1, names::PLAINS, 1),
                tapped(P1, names::PLAINS, 1),
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Library,
                    card: None,
                    count: 2,
                }),
            ]),
        // Overloaded, it exiles both of P1's creatures and spares P0's; P1 may find two basics
        // and finds the Island, then is done.
        Scenario::new("Winds of Abandon overloaded: a basic per creature exiled")
            .battlefield(P0, SIX_PLAINS_AND_ADELINE)
            .hand(P0, &[WINDS, names::PLAINS])
            .battlefield(P1, &[Permanent::new(SPELLBINDER), Permanent::new(TWO_DROP)])
            .library(P1, &[names::ISLAND, names::PLAINS, names::TUNDRA])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: WINDS,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                Action::Resolve,
                Action::choose(P1, DecisionKind::ChooseUpTo, names::ISLAND),
                Action::answer(P1, DecisionKind::ChooseUpTo, Pick::Done),
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Exile,
                    card: None,
                    count: 2,
                }),
                controls(P0, TWO_DROP, 1),
                controls(P1, names::ISLAND, 1),
                controls(P1, names::PLAINS, 0),
                tapped(P1, names::ISLAND, 1),
            ]),
        // A library with no basic land: the search finds nothing and asks nothing.
        Scenario::new("Winds of Abandon: no basic land to find")
            .battlefield(
                P0,
                &[Permanent::new(names::PLAINS), Permanent::new(names::PLAINS)],
            )
            .hand(P0, &[WINDS, names::PLAINS])
            .battlefield(P1, &[Permanent::new(SPELLBINDER)])
            .library(P1, &[names::TUNDRA, TWO_DROP])
            .actions(&[
                Action::cast(P0, WINDS),
                Action::Resolve,
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Exile,
                    card: Some(SPELLBINDER),
                    count: 1,
                }),
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Battlefield,
                    card: None,
                    count: 0,
                }),
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Library,
                    card: None,
                    count: 2,
                }),
            ]),
        // UI regression by-yfw: the token leaves while Winds stays on the stack for the search.
        Scenario::new("Winds of Abandon exiles a token before its controller searches")
            .interaction(&[names::STAFF_OF_THE_STORYTELLER])
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(
                P0,
                &[Permanent::new(names::PLAINS), Permanent::new(names::PLAINS)],
            )
            .hand(P0, &[names::WINDS_OF_ABANDON, names::PLAINS])
            .battlefield(
                P1,
                &[Permanent::new(names::PLAINS), Permanent::new(names::PLAINS)],
            )
            .hand(P1, &[names::STAFF_OF_THE_STORYTELLER])
            .library(P0, &[names::ISLAND, names::ISLAND])
            .library(P1, &[names::ISLAND, names::PLAINS])
            .actions(&[
                // Create the token in play: external engines do not share Beryl token names.
                Action::cast(P1, names::STAFF_OF_THE_STORYTELLER),
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::PrecombatMain,
                },
                Action::cast(P0, names::WINDS_OF_ABANDON),
                Action::Resolve,
                Action::choose(P1, DecisionKind::ChooseUpTo, names::ISLAND),
                Action::assert(Assertion::Controls {
                    player: P1,
                    card: "Spirit Token",
                    count: 0,
                }),
            ]),
    ];
}
