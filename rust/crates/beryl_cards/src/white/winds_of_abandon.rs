//! Winds of Abandon. {1}{W} Sorcery. "Exile target creature you don't control. For each
//! creature exiled this way, its controller searches their library for a basic land card.
//! Those players put those cards onto the battlefield tapped, then shuffle. Overload
//! {4}{W}{W} (You may cast this spell for its overload cost. If you do, change "target" in its
//! text to "each.")"

use beryl_engine::bitboard::Bitboard;
use beryl_engine::card::{
    Ability, AbilityKind, CardDef, CastingOption, Castings, Colors, Face, FaceIx, Flow, ManaCost,
    Supertypes, TargetKind, TargetSpec, Types,
};
use beryl_engine::effect::EffectCtx;
use beryl_engine::event::Reason;
use beryl_engine::filter::{Filter, Whose};
use beryl_engine::{DecisionKind, Seat, TurnStep, Zone};
use beryl_engine::{Layout, bodies};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

use crate::names;

const THEIR_CREATURES: Filter = Filter::All(&[
    Filter::Types(Types::CREATURE),
    Filter::Whose(Whose::Opponent),
]);

const BASIC_LANDS: Filter = Filter::All(&[
    Filter::Types(Types::LAND),
    Filter::Supertypes(Supertypes::BASIC),
]);

/// Exiles `creatures` at once, then each player whose creatures were exiled searches for that
/// many basic land cards ([CR 701.23a](https://yawgatog.com/resources/magic-rules/#R70123a))
/// and puts them onto the battlefield tapped, then shuffles
/// ([CR 701.24a](https://yawgatog.com/resources/magic-rules/#R70124a)). A player may find
/// fewer, or none from a library without basics
/// ([CR 701.23b](https://yawgatog.com/resources/magic-rules/#R70123b)): "that player isn't
/// required to find some or all of those cards". Who searches is each creature's controller
/// as it last existed on the battlefield: in exile it has none.
fn exile_and_fetch<L: Layout>(ctx: &mut EffectCtx<'_, L>, creatures: Bitboard<L::Slot>) -> Flow {
    let controlled = Seat::BOTH.map(|player| {
        creatures
            .iter()
            .filter(|&slot| ctx.state().controller(slot) == player)
            .collect::<Bitboard<L::Slot>>()
    });
    ctx.move_all(creatures, Zone::Exile, Reason::Exile);
    for player in Seat::BOTH {
        // A creature a replacement kept from exile was not "exiled this way".
        let n = controlled[player.index()]
            .iter()
            .filter(|&slot| ctx.state().object(slot).zone == Zone::Exile)
            .count();
        let Ok(n @ 1..) = u8::try_from(n) else {
            continue;
        };
        let found = ctx.search_library_up_to(player, BASIC_LANDS, n)?;
        ctx.put_onto_battlefield_tapped(found);
        ctx.shuffle(player);
    }
    Ok(())
}

/// "Exile target creature you don't control"
/// ([CR 701.13a](https://yawgatog.com/resources/magic-rules/#R70113a)): "To exile an object,
/// move it to the exile zone from wherever it is."
fn exile_target<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    let Some(creature) = ctx.target_object(0) else {
        return Ok(());
    };
    exile_and_fetch(ctx, Bitboard::single(creature.slot))
}

/// Overloaded, "Exile each creature you don't control"
/// ([CR 702.96a](https://yawgatog.com/resources/magic-rules/#R70296a)), with no target
/// ([CR 702.96b](https://yawgatog.com/resources/magic-rules/#R70296b)).
fn exile_each<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    let s = ctx.state();
    let theirs = THEIR_CREATURES.select(
        s,
        ctx.table(),
        s.battlefield().iter().copied().collect(),
        ctx.controller(),
    );
    exile_and_fetch(ctx, theirs)
}

pub static CARD: CardDef = CardDef {
    name: names::WINDS_OF_ABANDON,
    faces: &[Face {
        cost: ManaCost::new(1, [1, 0, 0, 0, 0, 0]),
        types: Types::SORCERY,
        colors: Colors::WHITE,
        abilities: &[
            Ability {
                kind: AbilityKind::Spell,
                targets: &[TargetSpec::one(TargetKind::Permanent, THEIR_CREATURES).removal()],
                may_pause: true,
                body: bodies!(exile_target),
                ..Ability::NONE
            },
            Ability {
                kind: AbilityKind::Spell,
                may_pause: true,
                body: bodies!(exile_each),
                ..Ability::NONE
            },
        ],
        casting: &[CastingOption::Overload {
            cost: ManaCost::new(4, [2, 0, 0, 0, 0, 0]),
            ability: 1,
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};

const WINDS: &str = names::WINDS_OF_ABANDON;
const SPELLBINDER: &str = names::ELITE_SPELLBINDER;
const SKYDIVER: &str = names::THIEVING_SKYDIVER;
const ADELINE: &str = names::ADELINE_RESPLENDENT_CATHAR;

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
    Permanent::new(ADELINE),
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
                Permanent::new(ADELINE),
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
            controls(P0, ADELINE, 1),
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
        .battlefield(P1, &[Permanent::new(SPELLBINDER), Permanent::new(SKYDIVER)])
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
            controls(P0, ADELINE, 1),
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
        .library(P1, &[names::TUNDRA, names::SEACHROME_COAST])
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
