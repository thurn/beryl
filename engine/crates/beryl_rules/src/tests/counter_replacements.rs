//! Replacement effects of counters being put on a permanent ([CR 616.1]): the order its
//! controller chooses when two rewrite the number differently, doubling the counters on a
//! permanent, and a number that does not fit.
//!
//! [CR 616.1]: https://yawgatog.com/resources/magic-rules/#R6161

use beryl_card::effect::build::{add_counters, double_counters, gain_life, permanents, you};
use beryl_card::phrases::creature_you_control;
use beryl_card::{Subtype, SubtypeWords, Subtypes};

use crate::card::{Ability, CardDef, Face, Purpose, Replacement, Rewrite, Stop, Types};
use crate::decision::{Choice, Decision, DecisionKind, Opt};
use crate::filter::Filter;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::{Seat, Slot};
use crate::interpret::Run;
use crate::replacement::event::Sink;
use crate::resolution::Resolution;
use crate::state::{AbilityRef, CounterKind, PendingTrigger, TurnStep, Zone};
use crate::zones::counters;
use crate::{CardTable, GameTable, Profile, Replay, Scratch, State, apply, continuous::layers};
use beryl_card::Players;
use beryl_kernel::format::Format;

use super::events::{put, with_game};
use super::{LAND, decide};

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

/// "If an effect would put one or more counters on a permanent you control, it puts twice that
/// many of those counters on that permanent instead."
static DOUBLER: CardDef = CardDef {
    name: "Test Counter Doubler",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ENCHANTMENT,
        abilities: &[Ability::replacing(&Replacement::counters_put(
            &Filter::Whose(&Players::You),
            None,
            true,
            Rewrite::Multiply(2),
        ))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "If one or more +1/+1 counters would be put on a creature you control, that many plus one
/// +1/+1 counters are put on it instead."
static SCALES: CardDef = CardDef {
    name: "Test Scales",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ENCHANTMENT,
        abilities: &[Ability::replacing(&Replacement::counters_put(
            &creature_you_control(),
            Some(CounterKind::PlusOne),
            false,
            Rewrite::Plus(1),
        ))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// Free sorcery: "Put a +1/+1 counter on each creature you control."
static GROW: CardDef = CardDef {
    name: "Test Grow",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::SORCERY,
        abilities: &[Ability::spell().effect(&add_counters(
            permanents(creature_you_control()),
            CounterKind::PlusOne,
            1,
        ))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// A Saga of four chapters, each gaining its controller 1 life.
static SAGA: CardDef = CardDef {
    name: "Test Long Saga",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ENCHANTMENT,
        subtypes: SubtypeWords::new(Subtypes::of(&[Subtype::Saga])),
        abilities: &[Ability::chapters::<1, 4>().effect(&gain_life(you(), 1))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "Double the number of +1/+1 counters on each creature you control."
static DOUBLE: beryl_card::Effect =
    double_counters(permanents(creature_you_control()), CounterKind::PlusOne);

static CARDS: CardTable = CardTable::new(&[&LAND, &BEAR, &DOUBLER, &SCALES, &GROW, &SAGA]);

const DECK: &[(u8, &str)] = &[
    (1, "Test Bear"),
    (1, "Test Counter Doubler"),
    (1, "Test Scales"),
    (1, "Test Grow"),
    (1, "Test Long Saga"),
    (9, "Test Land"),
];

/// A game in the first player's precombat main phase, with the named cards of the first player
/// put onto the battlefield in order.
fn game(names: &[&str]) -> (GameTable, State, Scratch, Vec<Slot>) {
    let t = GameTable::new(Format::CONSTRUCTED, &CARDS, [DECK, DECK]).unwrap();
    let mut s = State::new(&t, 3, Profile::EXACT).unwrap();
    s.board().begin_at(1, Seat::P0, TurnStep::PrecombatMain);
    let mut x = Scratch::new();
    let slots = names
        .iter()
        .map(|name| put(&t, &mut s, &mut x, Seat::P0, name))
        .collect();
    (t, s, x, slots)
}

/// Puts `n` +1/+1 counters on the permanent in `slot` by an effect.
fn add(t: &GameTable, s: &mut State, x: &mut Scratch, slot: Slot, n: u8) -> Result<(), Full> {
    let mut result = Ok(());
    with_game(t, s, x, slot, |game, _| {
        let permanent = game
            .state()
            .permanent(game.state().object_ref(slot))
            .unwrap();
        result = counters::add(game, permanent, CounterKind::PlusOne, n, true);
    });
    result
}

/// Carries out [`DOUBLE`] as an ability of the permanent in `slot` resolving.
fn double(t: &GameTable, s: &mut State, x: &mut Scratch, slot: Slot) -> Result<(), Stop> {
    static NO_ABILITY: Ability = Ability::spell();
    let ability = PendingTrigger {
        of: AbilityRef {
            source: Some(s.object_ref(slot)),
            card: s.copiable(slot).record(t).0,
            controller: s.controller(slot).into(),
            ..AbilityRef::default()
        },
        ..PendingTrigger::default()
    };
    let mut resolution = Resolution::of_trigger(&ability, &NO_ABILITY);
    let mut game = Game::new(s, t, &mut x.events);
    let result = Run::new(&mut game, &mut resolution).carry_out(&DOUBLE);
    crate::interpret::finish(&mut game, Ok(())).unwrap();
    result
}

fn plus_one(s: &State, slot: Slot) -> u8 {
    s.counters_of(slot, CounterKind::PlusOne)
}

/// The first decision other than priority once the first player casts `spell` and both pass.
fn cast_until_asked(t: &GameTable, s: &mut State, x: &mut Scratch, spell: Slot) -> Decision {
    s.move_object(&mut Sink::off(), spell, Zone::Hand, crate::zone::Place::Top)
        .unwrap();
    layers::refresh(s, t, &mut Sink::off()).unwrap();
    let d = decide(t, s);
    let cast = (0..d.len() as u16)
        .find(|&i| {
            matches!(
                d.candidates.option(Choice(i)),
                Some(Opt::Cast { slot, .. }) if slot == spell
            )
        })
        .unwrap();
    apply(s, t, x, &mut Replay, Choice(cast));
    loop {
        let d = decide(t, s);
        if d.kind != DecisionKind::Priority {
            return d;
        }
        let pass = (0..d.len() as u16)
            .find(|&i| matches!(d.candidates.option(Choice(i)), Some(Opt::Pass)))
            .unwrap();
        apply(s, t, x, &mut Replay, Choice(pass));
    }
}

/// With a doubling replacement and a "plus one" replacement, one +1/+1 counter an effect puts
/// on a creature asks its controller which applies first (CR 616.1): doubling first gives
/// three (1 × 2 + 1), the addition first gives four ((1 + 1) × 2).
#[test]
fn doubling_and_plus_one_ask_the_controller_and_give_three_or_four() {
    let (t, mut s, mut x, slots) = game(&["Test Counter Doubler", "Test Scales", "Test Bear"]);
    let [doubler, scales, bear] = slots[..] else {
        unreachable!()
    };
    let spell = super::events::find(&s, &t, Seat::P0, "Test Grow");
    let d = cast_until_asked(&t, &mut s, &mut x, spell);
    assert_eq!(
        (d.kind, d.prompt.purpose, d.player, d.len()),
        (DecisionKind::ChooseObject, Purpose::Replace, Seat::P0, 2)
    );
    for (first, expected) in [(doubler, 3), (scales, 4)] {
        let mut s = s;
        let pick = (0..d.len() as u16)
            .find(|&i| d.candidates.option(Choice(i)) == Some(Opt::Object(first)))
            .unwrap();
        apply(&mut s, &t, &mut x, &mut Replay, Choice(pick));
        assert_eq!(plus_one(&s, bear), expected);
    }
}

/// CR 701.10e: doubling the counters on a creature with three gives it three more, six; under
/// a doubling replacement the three it is given are doubled, nine.
#[test]
fn doubling_counters_gives_as_many_more_and_is_itself_replaced() {
    let (t, mut s, mut x, slots) = game(&["Test Bear"]);
    let bear = slots[0];
    add(&t, &mut s, &mut x, bear, 3).unwrap();
    let mut plain = s;
    double(&t, &mut plain, &mut x, bear).unwrap();
    assert_eq!(plus_one(&plain, bear), 6);

    put(&t, &mut s, &mut x, Seat::P0, "Test Counter Doubler");
    double(&t, &mut s, &mut x, bear).unwrap();
    assert_eq!(plus_one(&s, bear), 9);
}

/// A number of counters that does not fit in a byte is [`Full`] in `Small`, never 255:
/// doubling 200 lore counters (which do not alter power/toughness), and a replacement doubling
/// 200 counters put.
#[test]
fn two_hundred_counters_doubled_is_full_in_small() {
    static DOUBLE_LORE: beryl_card::Effect =
        double_counters(beryl_card::Objects::This, CounterKind::Lore);
    static NO_ABILITY: Ability = Ability::spell();
    let (t, mut s, mut x, slots) = game(&["Test Bear"]);
    let bear = slots[0];
    let mut doubled = s;
    let mut added = Ok(());
    with_game(&t, &mut doubled, &mut x, bear, |game, _| {
        let permanent = game
            .state()
            .permanent(game.state().object_ref(bear))
            .unwrap();
        added = counters::add(game, permanent, CounterKind::Lore, 200, true);
    });
    added.unwrap();
    let ability = PendingTrigger {
        of: AbilityRef {
            source: Some(doubled.object_ref(bear)),
            card: doubled.copiable(bear).record(&t).0,
            controller: doubled.controller(bear).into(),
            ..AbilityRef::default()
        },
        ..PendingTrigger::default()
    };
    let mut resolution = Resolution::of_trigger(&ability, &NO_ABILITY);
    let mut g = Game::new(&mut doubled, &t, &mut x.events);
    let result = Run::new(&mut g, &mut resolution).carry_out(&DOUBLE_LORE);
    assert_eq!(result, Err(Stop::Full));

    put(&t, &mut s, &mut x, Seat::P0, "Test Counter Doubler");
    assert_eq!(add(&t, &mut s, &mut x, bear, 200), Err(Full));
}

/// A Saga's first lore counter is put by its intrinsic replacement effect (CR 715.3a), so a
/// replacement of the counters effects put doubles it; the lore counter of its controller's
/// precombat main phase is a turn-based action (CR 715.3c), which the replacement leaves alone.
#[test]
fn a_saga_enters_with_doubled_lore_but_its_main_phase_counter_is_not_doubled() {
    let (t, mut s, mut x, slots) = game(&["Test Counter Doubler", "Test Long Saga"]);
    let saga = slots[1];
    assert_eq!(s.counters_of(saga, CounterKind::Lore), 2);
    crate::saga::advance(&mut s, &t, &mut x.events, Seat::P0).unwrap();
    assert_eq!(s.counters_of(saga, CounterKind::Lore), 3);
}
