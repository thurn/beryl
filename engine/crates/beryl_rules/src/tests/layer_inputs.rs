//! What makes the layer results stale besides the battlefield's own changes: a
//! characteristic-defining ability that reads the graveyards ([CR 604.3a]) is read again when a
//! card enters or leaves one, and only in a game where such a permanent is on the battlefield.
//!
//! [CR 604.3a]: https://yawgatog.com/resources/magic-rules/#R6043a

use beryl_card::{Amount, CounterKind, Objects, Players};

use crate::card::{Ability, CardDef, Face, LayerSpec, Modification, Types};
use crate::filter::Filter;
use crate::ids::{Seat, Slot};
use crate::replacement::event::Sink;
use crate::state::{TurnStep, Zone};
use crate::table::Capability;
use crate::zone::Place;
use crate::{CardTable, GameTable, Profile, State, continuous::layers};
use beryl_kernel::format::Format;

use super::events::find;
use super::{LAND, SPELL_A, SPELL_B};

/// "The number of card types among cards in all graveyards".
static CARD_TYPES: Amount = Amount::CardTypesAmong(&Objects::All {
    zone: Zone::Graveyard,
    filter: Filter::Any,
});

/// "This creature's power is equal to the number of card types among cards in all graveyards
/// and its toughness is equal to that number plus 1."
static GOYF: CardDef = CardDef {
    name: "Test Goyf",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(0),
        toughness: Some(1),
        abilities: &[Ability::static_ability(&LayerSpec {
            affected: Objects::This,
            condition: None,
            modifications: &[Modification::SetPt(
                &CARD_TYPES,
                &Amount::Plus(1, &CARD_TYPES),
            )],
        })],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static GOYF_CARDS: CardTable = CardTable::new(&[&LAND, &SPELL_A, &SPELL_B, &GOYF]);

/// A game of the test cards in which each player's library holds a Goyf, a sorcery, an
/// instant and lands, with the layer results current.
fn game() -> (GameTable, State) {
    let deck: &[(u8, &str)] = &[
        (1, "Test Goyf"),
        (2, "Test Spell A"),
        (2, "Test Spell B"),
        (5, "Test Land"),
    ];
    let t = GameTable::new(Format::CONSTRUCTED, &GOYF_CARDS, [deck, deck]).unwrap();
    let mut s = State::new(&t, 3, Profile::EXACT).unwrap();
    s.board().begin_at(1, Seat::P0, TurnStep::PrecombatMain);
    layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();
    (t, s)
}

/// Moves a card of `player`'s called `name` from their library to their graveyard through the
/// zone move, as the rules move it, and says whether the layer results are stale after.
fn mill(s: &mut State, t: &GameTable, player: Seat, name: &str) -> (Slot, bool) {
    let slot = find(s, t, player, name);
    s.move_object(&mut Sink::off(), slot, Zone::Graveyard, Place::Top)
        .unwrap();
    (slot, s.is_derived_stale())
}

/// With no permanent that reads the graveyards on the battlefield, a card that enters one
/// changes no layer result, so the results stay current and no recompute runs.
#[test]
fn a_card_entering_a_graveyard_marks_nothing_stale_with_no_reader() {
    let (t, mut s) = game();
    let (_, stale) = mill(&mut s, &t, Seat::P0, "Test Spell A");
    assert!(!stale);
    // The Goyf in the library reads nothing: it is not on the battlefield.
    let (_, stale) = mill(&mut s, &t, Seat::P1, "Test Land");
    assert!(!stale);
}

/// With the Goyf on the battlefield, each card that enters or leaves a graveyard marks the
/// results stale, and the Goyf counts the card types among them: a second card of a type adds
/// nothing, and the other player's graveyard counts too ([CR 613.4a]): "Layer 7a: Effects from
/// characteristic-defining abilities that define power and/or toughness are applied."
///
/// [CR 613.4a]: https://yawgatog.com/resources/magic-rules/#R6134a
#[test]
fn a_reader_on_the_battlefield_is_recomputed_as_graveyards_change() {
    let (t, mut s) = game();
    let goyf = find(&s, &t, Seat::P0, "Test Goyf");
    s.board().place(goyf, Zone::Battlefield).unwrap();
    let refresh = |s: &mut State| layers::refresh(s, &t, &mut Sink::off()).unwrap();
    refresh(&mut s);
    let pt = |s: &State| (s.derived(goyf).power, s.derived(goyf).toughness);
    assert_eq!(pt(&s), (0, 1));

    let (sorcery, stale) = mill(&mut s, &t, Seat::P0, "Test Spell A");
    assert!(stale);
    refresh(&mut s);
    assert_eq!(pt(&s), (1, 2));

    let (_, stale) = mill(&mut s, &t, Seat::P0, "Test Spell A");
    assert!(stale);
    refresh(&mut s);
    assert_eq!(pt(&s), (1, 2));

    let (_, stale) = mill(&mut s, &t, Seat::P1, "Test Land");
    assert!(stale);
    refresh(&mut s);
    assert_eq!(pt(&s), (2, 3));

    // Leaving a graveyard is read too: one sorcery is still there.
    s.move_object(&mut Sink::off(), sorcery, Zone::Exile, Place::Top)
        .unwrap();
    assert!(s.is_derived_stale());
    refresh(&mut s);
    assert_eq!(pt(&s), (2, 3));

    // Once the Goyf has left the battlefield, nothing reads the graveyards.
    s.move_object(&mut Sink::off(), goyf, Zone::Hand, Place::Top)
        .unwrap();
    refresh(&mut s);
    let (_, stale) = mill(&mut s, &t, Seat::P1, "Test Spell B");
    assert!(!stale);
}

/// "Creatures you control with +1/+1 counters on them get +1/+1."
static PLUS_ONE_LORD: CardDef = CardDef {
    name: "Test Counter Lord",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ENCHANTMENT,
        abilities: &[Ability::static_ability(&LayerSpec {
            affected: Objects::All {
                zone: Zone::Battlefield,
                filter: Filter::All(&[
                    Filter::Types(Types::CREATURE),
                    Filter::Whose(&Players::You),
                    Filter::WithCounter(CounterKind::PlusOne),
                ]),
            },
            condition: None,
            modifications: &[Modification::ModifyPt(1, 1)],
        })],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "Creatures you control with charge counters on them get +1/+1."
static CHARGE_LORD: CardDef = CardDef {
    name: "Test Charge Lord",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ENCHANTMENT,
        abilities: &[Ability::static_ability(&LayerSpec {
            affected: Objects::All {
                zone: Zone::Battlefield,
                filter: Filter::All(&[
                    Filter::Types(Types::CREATURE),
                    Filter::Whose(&Players::You),
                    Filter::WithCounter(CounterKind::Charge),
                ]),
            },
            condition: None,
            modifications: &[Modification::ModifyPt(1, 1)],
        })],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// A 2/2 creature with no abilities.
static TEST_BEAR: CardDef = CardDef {
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

static COUNTER_CARDS: CardTable =
    CardTable::new(&[&LAND, &TEST_BEAR, &PLUS_ONE_LORD, &CHARGE_LORD]);

/// A permanent whose static ability filters by [`Filter::WithCounter`] gives a creature the
/// bonus as soon as a counter of that kind is put on it, and removes the bonus when the
/// counter is removed, with no other change to the board.
#[test]
fn with_counter_filter_updates_when_counters_are_added_and_removed() {
    let deck: &[(u8, &str)] = &[
        (1, "Test Counter Lord"),
        (1, "Test Charge Lord"),
        (2, "Test Bear"),
        (6, "Test Land"),
    ];
    let t = GameTable::new(Format::CONSTRUCTED, &COUNTER_CARDS, [deck, deck]).unwrap();
    assert!(t.can(Capability::LayerInputs));
    let mut s = State::new(&t, 3, Profile::EXACT).unwrap();
    s.board().begin_at(1, Seat::P0, TurnStep::PrecombatMain);

    let plus_lord = find(&s, &t, Seat::P0, "Test Counter Lord");
    let charge_lord = find(&s, &t, Seat::P0, "Test Charge Lord");
    let bear = find(&s, &t, Seat::P0, "Test Bear");
    s.board().place(plus_lord, Zone::Battlefield).unwrap();
    s.board().place(charge_lord, Zone::Battlefield).unwrap();
    s.board().place(bear, Zone::Battlefield).unwrap();
    let refresh = |s: &mut State| layers::refresh(s, &t, &mut Sink::off()).unwrap();
    refresh(&mut s);
    let pt = |s: &State| (s.derived(bear).power, s.derived(bear).toughness);
    assert_eq!(pt(&s), (2, 2));

    // Putting a +1/+1 counter on the bear marks the layers stale: the counter itself gives
    // +1/+1 and the lord's Filter::WithCounter gives another +1/+1.
    let current = s.current(s.object_ref(bear)).unwrap();
    let layered = s.layers_read_counters(&t, bear, CounterKind::PlusOne);
    assert!(layered);
    let _ = s
        .add_counters(current, CounterKind::PlusOne, 1, layered)
        .unwrap();
    assert!(s.is_derived_stale());
    refresh(&mut s);
    assert_eq!(pt(&s), (4, 4));

    // Removing the +1/+1 counter marks the layers stale and drops both bonuses.
    let current = s.current(s.object_ref(bear)).unwrap();
    let _ = s.remove_counters(current, CounterKind::PlusOne, 1, false);
    assert!(s.is_derived_stale());
    refresh(&mut s);
    assert_eq!(pt(&s), (2, 2));

    // A non-P/T counter (charge) also marks the layers stale while ANY_COUNTERS is read,
    // even when the caller passes `layered = false`.
    let current = s.current(s.object_ref(bear)).unwrap();
    assert!(s.layers_read_counters(&t, bear, CounterKind::Charge));
    let _ = s
        .add_counters(current, CounterKind::Charge, 1, false)
        .unwrap();
    assert!(s.is_derived_stale());
    refresh(&mut s);
    assert_eq!(pt(&s), (3, 3));

    let current = s.current(s.object_ref(bear)).unwrap();
    let _ = s.remove_counters(current, CounterKind::Charge, 1, false);
    assert!(s.is_derived_stale());
    refresh(&mut s);
    assert_eq!(pt(&s), (2, 2));

    // Once both lords leave the battlefield, adding a charge counter marks nothing stale.
    s.move_object(&mut Sink::off(), plus_lord, Zone::Hand, Place::Top)
        .unwrap();
    s.move_object(&mut Sink::off(), charge_lord, Zone::Hand, Place::Top)
        .unwrap();
    refresh(&mut s);
    let current = s.current(s.object_ref(bear)).unwrap();
    assert!(!s.layers_read_counters(&t, bear, CounterKind::Charge));
    let _ = s
        .add_counters(current, CounterKind::Charge, 1, false)
        .unwrap();
    assert!(!s.is_derived_stale());
}

/// A game with no permanent that reads counters or other layer inputs does not enable
/// [`Capability::LayerInputs`] and does not mark the layers stale when a non-P/T counter is
/// added or removed.
#[test]
fn adding_a_counter_marks_nothing_stale_without_layer_inputs() {
    let deck: &[(u8, &str)] = &[(4, "Test Bear"), (6, "Test Land")];
    let t = GameTable::new(Format::CONSTRUCTED, &COUNTER_CARDS, [deck, deck]).unwrap();
    assert!(!t.can(Capability::LayerInputs));
    let mut s = State::new(&t, 3, Profile::EXACT).unwrap();
    s.board().begin_at(1, Seat::P0, TurnStep::PrecombatMain);
    let bear = find(&s, &t, Seat::P0, "Test Bear");
    s.board().place(bear, Zone::Battlefield).unwrap();
    layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();

    let current = s.current(s.object_ref(bear)).unwrap();
    let layered = s.layers_read_counters(&t, bear, CounterKind::Charge);
    assert!(!layered);
    let _ = s
        .add_counters(current, CounterKind::Charge, 1, layered)
        .unwrap();
    assert!(!s.is_derived_stale());

    let current = s.current(s.object_ref(bear)).unwrap();
    let _ = s.remove_counters(current, CounterKind::Charge, 1, layered);
    assert!(!s.is_derived_stale());
}
