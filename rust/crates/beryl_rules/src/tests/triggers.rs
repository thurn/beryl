//! A trigger is an event, a subject and players (`beryl_card::Trigger`): the products no
//! printed card of the pool says, each matched by the code that matches the pool's.

// Bodies share the `Flow` signature of `Ability::body`, whether or not they can pause.
#![allow(clippy::unnecessary_wraps)]

use super::begin;
use super::events::{advance_to, put, resolve_all};
use crate::bitboard::Bitboard;
use crate::card::{
    Ability, CardDef, Castings, Face, Flow, Players, Subject, Subtype, Subtypes, Trigger,
    TriggerEvent, Types,
};
use crate::effect::EffectCtx;
use crate::event::{self, Event};
use crate::filter::Filter;
use crate::fixed::Fixed;
use crate::ids::Seat;
use crate::state::{TurnStep, Zone, ZoneKey};
use crate::zone::Place;
use crate::{CardTable, Decklist, GameTable, Profile, Scratch, State};
use crate::{Layout, bodies};
use beryl_kernel::two_player::TwoPlayerSeat as _;

fn gain_one<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    ctx.gain_life(ctx.controller(), 1);
    Ok(())
}

/// A permanent with one triggered ability: "[trigger], you gain 1 life."
macro_rules! listener {
    ($name:ident, $text:literal, $trigger:expr) => {
        static $name: CardDef = CardDef {
            name: $text,
            faces: &[Face {
                types: Types::ARTIFACT,
                abilities: &[Ability::triggered(&$trigger, bodies!(gain_one))],
                ..Face::EMPTY
            }],
            tokens: &[],
        };
    };
}

static LAND: CardDef = CardDef {
    name: "Test Land",
    faces: &[Face {
        types: Types::LAND,
        ..Face::EMPTY
    }],
    tokens: &[],
};
static BEAR: CardDef = CardDef {
    name: "Test Bear",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(2),
        toughness: Some(2),
        ..Face::EMPTY
    }],
    tokens: &[],
};
static KNIGHT: CardDef = CardDef {
    name: "Test Knight",
    faces: &[Face {
        types: Types::CREATURE,
        subtypes: Subtypes::of(&[Subtype::Knight]),
        power: Some(2),
        toughness: Some(2),
        ..Face::EMPTY
    }],
    tokens: &[],
};
// "Whenever an opponent casts a spell".
listener!(
    HECKLER,
    "Test Heckler",
    Trigger::casts(Players::EachOpponent, Filter::Any)
);
// "Whenever you cast a noncreature spell".
listener!(
    PURIST,
    "Test Purist",
    Trigger::casts(Players::You, Filter::Not(&Filter::Types(Types::CREATURE)))
);
// "At the beginning of each player's upkeep".
listener!(
    SANCTUARY,
    "Test Sanctuary",
    Trigger::step(TurnStep::Upkeep, Players::EachPlayer)
);
// "At the beginning of each of your main phases".
listener!(
    METRONOME,
    "Test Metronome",
    Trigger::main_phase(Players::You)
);
// "Whenever a player draws a card".
listener!(ARCHIVE, "Test Archive", Trigger::draws(Players::EachPlayer));
// "Whenever you attack with one or more Knights".
listener!(
    MARSHAL,
    "Test Marshal",
    Trigger::of(
        TriggerEvent::PlayerAttacks,
        Subject::A(Filter::Subtype(Subtype::Knight)),
    )
    .whose(Players::You)
);

static CARDS: CardTable = CardTable::new(&[
    &LAND, &BEAR, &KNIGHT, &HECKLER, &PURIST, &SANCTUARY, &METRONOME, &ARCHIVE, &MARSHAL,
]);

const DECK: &Decklist = &[
    (2, "Test Bear"),
    (2, "Test Knight"),
    (1, "Test Heckler"),
    (1, "Test Purist"),
    (1, "Test Sanctuary"),
    (1, "Test Metronome"),
    (1, "Test Archive"),
    (1, "Test Marshal"),
    (31, "Test Land"),
];

/// A game in the starting player's first main phase, each player with a land in hand so
/// that every main phase of theirs is a decision.
fn game() -> (GameTable, State, Seat) {
    let t = GameTable::new(&CARDS, [DECK, DECK]).unwrap();
    let mut s = State::new(&t, 7, Profile::EXACT).unwrap();
    begin(&t, &mut s);
    for player in Seat::BOTH {
        let land = s
            .in_candidate_order(s.zone(ZoneKey::Of(player, Zone::Library)))
            .find(|&slot| s.definition_name(&t, slot) == "Test Land")
            .unwrap();
        s.move_object(&mut event::Sink::off(), land, Zone::Hand, Place::Top)
            .unwrap();
    }
    let p = s.turn().starting_player();
    (t, s, p)
}

/// `player` casting `name`, as the event of it.
fn cast(t: &GameTable, s: &State, player: Seat, name: &str) -> Event {
    Event::Cast {
        spell: s.object_ref(super::events::find(s, t, player, name)),
        controller: player,
        targets: Fixed::default(),
        x: 0,
        costs: Castings::default(),
        flashback: false,
    }
}

#[test]
fn a_cast_trigger_is_for_the_players_it_names() {
    let (t, mut s, p) = game();
    let q = p.opponent();
    let mut x = Scratch::new();
    put(&t, &mut s, &mut x, p, "Test Heckler");
    let mine = cast(&t, &s, p, "Test Bear");
    event::settle(&mut s, &t, &mut x.events, mine).unwrap();
    assert!(s.triggers().is_empty(), "not its controller's spell");
    let theirs = cast(&t, &s, q, "Test Bear");
    event::settle(&mut s, &t, &mut x.events, theirs).unwrap();
    assert_eq!(s.triggers().len(), 1);
    assert_eq!(s.triggers()[0].info.player, q, "the player who cast it");
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.player(p).life(), 21);
}

#[test]
fn a_cast_trigger_reads_its_subject_of_the_spell() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    put(&t, &mut s, &mut x, p, "Test Purist");
    let creature = cast(&t, &s, p, "Test Bear");
    event::settle(&mut s, &t, &mut x.events, creature).unwrap();
    assert!(s.triggers().is_empty(), "a creature spell");
    let artifact = cast(&t, &s, p, "Test Sanctuary");
    event::settle(&mut s, &t, &mut x.events, artifact).unwrap();
    assert_eq!(s.triggers().len(), 1);
    let theirs = cast(&t, &s, p.opponent(), "Test Sanctuary");
    event::settle(&mut s, &t, &mut x.events, theirs).unwrap();
    assert_eq!(s.triggers().len(), 1, "not an opponent's");
}

#[test]
fn a_step_trigger_for_each_player_fires_in_every_turn() {
    let (t, mut s, p) = game();
    let q = p.opponent();
    let mut x = Scratch::new();
    put(&t, &mut s, &mut x, p, "Test Sanctuary");
    advance_to(&t, &mut s, &mut x, q, TurnStep::PrecombatMain);
    assert_eq!(s.player(p).life(), 21, "the opponent's upkeep");
    advance_to(&t, &mut s, &mut x, p, TurnStep::PrecombatMain);
    assert_eq!(s.player(p).life(), 22, "and its controller's");
}

#[test]
fn a_main_phase_trigger_fires_in_both_main_phases_of_a_turn() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    put(&t, &mut s, &mut x, p, "Test Metronome");
    advance_to(&t, &mut s, &mut x, p, TurnStep::PostcombatMain);
    assert_eq!(s.player(p).life(), 21);
    // The opponent's two main phases are none of its controller's.
    advance_to(&t, &mut s, &mut x, p.opponent(), TurnStep::PostcombatMain);
    assert_eq!(s.player(p).life(), 21);
    advance_to(&t, &mut s, &mut x, p, TurnStep::PostcombatMain);
    assert_eq!(s.player(p).life(), 23);
}

#[test]
fn a_draw_trigger_fires_once_for_each_card_whoever_draws() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    put(&t, &mut s, &mut x, p, "Test Archive");
    for (player, cards) in [(p, 1), (p.opponent(), 2)] {
        event::settle(&mut s, &t, &mut x.events, Event::Draw { player, cards }).unwrap();
    }
    let drew: Vec<Seat> = s.triggers().iter().map(|tr| tr.info.player).collect();
    assert_eq!(drew, [p, p.opponent(), p.opponent()]);
}

#[test]
fn an_attack_trigger_counts_the_attackers_its_subject_accepts() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    put(&t, &mut s, &mut x, p, "Test Marshal");
    let bear = put(&t, &mut s, &mut x, p, "Test Bear");
    let knights = [
        put(&t, &mut s, &mut x, p, "Test Knight"),
        put(&t, &mut s, &mut x, p, "Test Knight"),
    ];
    let attack = |attackers: Bitboard<_>| Event::Attack {
        player: p,
        attackers,
    };
    event::settle(&mut s, &t, &mut x.events, attack(Bitboard::single(bear))).unwrap();
    assert!(s.triggers().is_empty(), "no Knight attacks");
    let all: Bitboard<_> = knights.into_iter().chain([bear]).collect();
    event::settle(&mut s, &t, &mut x.events, attack(all)).unwrap();
    assert_eq!(s.triggers().len(), 1, "once, however many");
    assert_eq!(s.triggers()[0].info.amount, 2, "the Knights, not the Bear");
}
