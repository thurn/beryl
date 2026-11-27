//! A player whose profile passes on identical triggers (`Profile::passes_identical_triggers`)
//! is asked once before a run of the same trigger resolves, not before each one: asked again
//! when a different trigger or anything put on the stack comes next, and every time when
//! their own flag is off.

use beryl_card::effect::build::{gain_life, you};

use super::begin;
use crate::card::ManaColor;
use crate::card::{Ability, CardDef, Castings, Face, ManaCost, Players, Trigger, Types};
use crate::decision::{Choice, DecisionKind, Opt, Step};
use crate::filter::Filter;
use crate::fixed::Fixed;
use crate::ids::Seat;
use crate::replacement::event::{self, Event, Reason};
use crate::state::{StackEntry, Zone, ZoneKey};
use crate::zone::Place;
use crate::{CardTable, Decklist, GameTable, Profile, Replay, Scratch, State, apply, next};
use beryl_kernel::format::Format;
use beryl_kernel::two_player::TwoPlayerSeat as _;

/// An artifact with "Whenever you cast a creature spell, you gain 1 life."
macro_rules! watcher {
    ($name:ident, $text:literal) => {
        static $name: CardDef = CardDef {
            name: $text,
            shape: beryl_card::definition::Shape::Single,
            faces: &[Face {
                types: Types::ARTIFACT,
                abilities: &[Ability::triggered(&Trigger::casts(
                    Players::You,
                    Filter::Types(Types::CREATURE),
                ))
                .effect(&gain_life(you(), 1))],
                ..Face::EMPTY
            }],
            tokens: &[],
        };
    };
}

watcher!(WATCHER, "Test Watcher");
watcher!(OTHER_WATCHER, "Test Other Watcher");

static LAND: CardDef = CardDef {
    name: "Test Land",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::LAND,
        ..Face::EMPTY
    }],
    tokens: &[],
};
static BEAR: CardDef = CardDef {
    name: "Test Bear",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(2),
        toughness: Some(2),
        ..Face::EMPTY
    }],
    tokens: &[],
};
/// An instant costing {1}: "You gain 1 life." Something to do at each priority.
static SPARK: CardDef = CardDef {
    name: "Test Spark",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        cost: ManaCost::new(1, [0; 6]),
        types: Types::INSTANT,
        abilities: &[Ability::spell().effect(&gain_life(you(), 1))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[&LAND, &BEAR, &SPARK, &WATCHER, &OTHER_WATCHER]);

const DECK: &Decklist = &[
    (4, "Test Watcher"),
    (1, "Test Other Watcher"),
    (2, "Test Bear"),
    (2, "Test Spark"),
    (31, "Test Land"),
];

/// A slot holding `name` in the player's library or hand.
fn find(s: &State, t: &GameTable, player: Seat, name: &str) -> crate::ids::Slot {
    let cards =
        s.zone(ZoneKey::Of(player, Zone::Library)) | s.zone(ZoneKey::Of(player, Zone::Hand));
    crate::tests::fixture_slots(cards)
        .find(|&slot| s.definition_name(t, slot) == name)
        .unwrap_or_else(|| panic!("no {name}"))
}

/// The starting player's first main phase under `profile`, with `watchers` on their
/// battlefield (by name, in the order they entered) all triggered by one creature spell they
/// cast, and each player holding a Test Spark with {1} in their pool to cast it.
fn triggered(watchers: &[&str], profile: Profile) -> (GameTable, State, Seat) {
    let t = GameTable::new(Format::CONSTRUCTED, &CARDS, [DECK, DECK]).unwrap();
    let mut s = State::new(&t, 7, Profile::EXACT).unwrap();
    begin(&t, &mut s);
    let p = s.turn().starting_player();
    let mut x = Scratch::new();
    for player in Seat::BOTH {
        let spark = find(&s, &t, player, "Test Spark");
        s.move_object(&mut event::Sink::off(), spark, Zone::Hand, Place::Top)
            .unwrap();
        let _mana = s.add_mana(player, None, ManaColor::Colorless, 1);
    }
    for name in watchers {
        let slot = find(&s, &t, p, name);
        event::move_one(
            &mut s,
            &t,
            &mut x.events,
            slot,
            Zone::Battlefield,
            Reason::Effect,
        )
        .unwrap();
    }
    let cast = Event::Cast {
        spell: s.object_ref(find(&s, &t, p, "Test Bear")),
        controller: p,
        targets: Fixed::default(),
        x: 0,
        costs: Castings::default(),
        flashback: false,
        from: Zone::Hand,
    };
    event::settle(&mut s, &t, &mut x.events, cast).unwrap();
    assert_eq!(s.triggers().len(), watchers.len());
    s.set_profile(profile);
    (t, s, p)
}

/// Plays until the stack and the waiting triggers are empty, answering each priority with
/// `respond` and every other decision with its first answer, and returns how many priority
/// decisions each seat was asked while the stack held something, and the most entries the
/// stack held.
fn asked(
    t: &GameTable,
    s: &mut State,
    mut respond: impl FnMut(&State, &crate::decision::Decision) -> Choice,
) -> ([usize; 2], usize) {
    let mut x = Scratch::new();
    let mut asked = [0; 2];
    let mut deepest = 0;
    loop {
        let Step::Decide(d) = next(s, t, &mut x, &mut Replay) else {
            panic!("the game ended");
        };
        if s.stack().is_empty() && s.triggers().is_empty() {
            return (asked, deepest);
        }
        deepest = deepest.max(s.stack().len());
        let c = if d.kind == DecisionKind::Priority {
            asked[d.player.index()] += 1;
            respond(s, &d)
        } else {
            Choice(0)
        };
        apply(s, t, &mut x, &mut Replay, c);
    }
}

/// A priority decision's first candidate is passing.
fn pass(_: &State, d: &crate::decision::Decision) -> Choice {
    assert_eq!(d.candidates.option(Choice(0)), Some(Opt::Pass));
    Choice(0)
}

const FOUR: &[&str] = &["Test Watcher"; 4];

/// The smart stops with passing on identical triggers by seat.
fn smart(passes: [bool; 2]) -> Profile {
    Profile {
        passes_identical_triggers: passes,
        ..Profile::SMART
    }
}

#[test]
fn four_identical_triggers_ask_each_player_once() {
    let (t, mut s, _) = triggered(FOUR, smart([true; 2]));
    let (asked, deepest) = asked(&t, &mut s, pass);
    assert_eq!(deepest, 4);
    assert_eq!(asked, [1, 1]);
}

#[test]
fn without_the_flag_each_trigger_asks() {
    let (t, mut s, _) = triggered(FOUR, smart([false; 2]));
    let (asked, _) = asked(&t, &mut s, pass);
    assert_eq!(asked, [4, 4]);
}

#[test]
fn a_seat_without_the_flag_is_asked_each_time() {
    let (t, mut s, p) = triggered(FOUR, smart([true; 2]));
    let mut passes = [true; 2];
    passes[p.opponent().index()] = false;
    s.set_profile(smart(passes));
    let (asked, _) = asked(&t, &mut s, pass);
    assert_eq!(asked[p.index()], 1);
    assert_eq!(asked[p.opponent().index()], 4);
}

/// A different trigger between identical ones asks again: each player is asked once for
/// each run of the same trigger on the stack.
#[test]
fn a_different_trigger_asks_again() {
    let watchers = [
        "Test Watcher",
        "Test Watcher",
        "Test Other Watcher",
        "Test Watcher",
    ];
    let (t, mut s, _) = triggered(&watchers, smart([true; 2]));
    let mut runs = 0;
    let (asked, _) = asked(&t, &mut s, |s, d| {
        if runs == 0 {
            // The whole stack, top first: one run per change of card.
            let cards: Vec<_> = s.stack().iter().rev().map(StackEntry::card).collect();
            runs = 1 + cards.windows(2).filter(|w| w[0] != w[1]).count();
        }
        pass(s, d)
    });
    assert!(runs >= 2, "the other trigger splits the stack");
    assert_eq!(asked, [runs, runs]);
}

/// A spell cast in response asks again: the active player passes on the first trigger, their
/// opponent answers it with an instant, and once the instant resolves the next trigger is a
/// decision again for both.
#[test]
fn a_spell_cast_in_response_asks_again() {
    let (t, mut s, p) = triggered(FOUR, smart([true; 2]));
    let q = p.opponent();
    let mut cast = false;
    let (asked, _) = asked(&t, &mut s, |s, d| {
        if d.player == q && !cast {
            cast = true;
            let spark = (0..d.len() as u16)
                .map(Choice)
                .find(|&c| matches!(d.candidates.option(c), Some(Opt::Cast { .. })))
                .expect("the instant is castable");
            return spark;
        }
        pass(s, d)
    });
    assert!(cast);
    // The active player: the first trigger, the instant, then the second trigger.
    assert_eq!(asked[p.index()], 3);
    // Their opponent, asked on the first trigger, has nothing left to do once they cast, so
    // each later priority of theirs passes silently.
    assert_eq!(asked[q.index()], 1);
}
