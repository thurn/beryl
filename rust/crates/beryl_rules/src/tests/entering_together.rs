//! Timestamps of permanents that enter the battlefield together ([CR 613.7m]): "their
//! relative timestamps are determined in APNAP order (see rule 101.4). Objects controlled by
//! the active player ... have an earlier relative timestamp in the order of that player's
//! choice, followed by each other player in turn order." The choice is asked only where the
//! order could change the layer results.
//!
//! [CR 613.7m]: https://yawgatog.com/resources/magic-rules/#R6137m

use beryl_card::{Colors, Objects};

use super::decide;
use super::events::find;
use crate::bitboard::Bitboard;
use crate::card::{Ability, CardDef, Face, LayerSpec, Modification, Purpose, Types};
use crate::filter::Filter;
use crate::ids::{Seat, Slot};
use crate::replacement::event::{self, Entry, Reason, Sink};
use crate::state::{Pending, TurnStep, Zone};
use crate::{CardTable, Choice, DecisionKind, GameTable, Profile, Replay, Scratch, State, apply};

/// A 2/2 creature.
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

/// An enchantment with one static ability over every creature, making `modification`.
macro_rules! over_creatures {
    ($name:literal, $modification:expr) => {
        CardDef {
            name: $name,
            faces: &[Face {
                types: Types::ENCHANTMENT,
                abilities: &[Ability::static_ability(&LayerSpec {
                    affected: Objects::All {
                        zone: Zone::Battlefield,
                        filter: Filter::Types(Types::CREATURE),
                    },
                    condition: None,
                    modifications: &[$modification],
                })],
                ..Face::EMPTY
            }],
            tokens: &[],
        }
    };
}

/// "Creatures are blue", in layer 5.
static BLUE: CardDef = over_creatures!("Test Blue", Modification::SetColors(Colors::BLUE));
/// "Creatures are red", in layer 5.
static RED: CardDef = over_creatures!("Test Red", Modification::SetColors(Colors::RED));
/// "Creatures get +1/+1", in layer 7c.
static ANTHEM: CardDef = over_creatures!("Test Anthem", Modification::ModifyPt(1, 1));
/// "Creatures get +1/+0", in layer 7c.
static SPUR: CardDef = over_creatures!("Test Spur", Modification::ModifyPt(1, 0));

static CARDS: CardTable = CardTable::new(&[&BEAR, &BLUE, &RED, &ANTHEM, &SPUR]);

/// A game in the first player's main phase, with a bear of theirs on the battlefield.
fn game() -> (GameTable, State, Slot) {
    let deck: &[(u8, &str)] = &[
        (4, "Test Bear"),
        (4, "Test Blue"),
        (4, "Test Red"),
        (4, "Test Anthem"),
        (4, "Test Spur"),
    ];
    let t = GameTable::new(&CARDS, [deck, deck]).unwrap();
    let mut s = State::new(&t, 3, Profile::EXACT).unwrap();
    s.board().begin_at(1, Seat::P0, TurnStep::PrecombatMain);
    let bear = find(&s, &t, Seat::P0, "Test Bear");
    s.board().place(bear, Zone::Battlefield).unwrap();
    (t, s, bear)
}

/// Exiles each of `cards`, the player and the name of each, then moves them all onto the
/// battlefield together under their owners' control, returning them in the order given.
fn enter_together<const N: usize>(
    t: &GameTable,
    s: &mut State,
    cards: [(Seat, &str); N],
) -> [Slot; N] {
    let slots = cards.map(|(player, name)| {
        let slot = find(s, t, player, name);
        s.board().place(slot, Zone::Exile).unwrap();
        slot
    });
    let together: Bitboard<Slot> = slots.iter().copied().collect();
    let entry = Entry::default();
    event::move_batch(
        s,
        t,
        &mut Sink::off(),
        together,
        Zone::Battlefield,
        Reason::Effect,
        entry,
    )
    .unwrap();
    slots
}

/// Two effects that set a creature's color, from permanents of one player that entered
/// together, apply in the order the player chooses for their timestamps: the one picked
/// first applies first, and the other's color is the one the bear ends with.
#[test]
fn the_controller_orders_permanents_that_entered_together_when_it_matters() {
    for (first, last, color) in [
        ("Test Red", "Test Blue", Colors::BLUE),
        ("Test Blue", "Test Red", Colors::RED),
    ] {
        let (t, mut s, bear) = game();
        let [blue, red] = enter_together(
            &t,
            &mut s,
            [(Seat::P0, "Test Blue"), (Seat::P0, "Test Red")],
        );
        let d = decide(&t, &mut s);
        assert_eq!(
            (d.kind, d.prompt.purpose, d.player, d.len()),
            (DecisionKind::ChooseObject, Purpose::EnterOrder, Seat::P0, 2)
        );
        let pick = if first == "Test Red" { red } else { blue };
        let choice = (0..2)
            .find(|&i| d.candidates.object(Choice(i)) == Some(pick))
            .unwrap();
        apply(&mut s, &t, &mut Scratch::new(), &mut Replay, Choice(choice));
        assert_eq!(*s.pending(), Pending::None, "{first} then {last}");
        crate::continuous::layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();
        assert!(
            s.object(pick).timestamp() < s.object(if pick == red { blue } else { red }).timestamp()
        );
        assert_eq!(s.derived(bear).colors, color, "{first} then {last}");
    }
}

/// No order is asked where none could change anything: effects in different layers, and
/// two that only add to power and toughness, which give the same result in either order.
#[test]
fn no_order_is_asked_when_the_order_cannot_change_the_results() {
    let (t, mut s, bear) = game();
    enter_together(
        &t,
        &mut s,
        [
            (Seat::P0, "Test Blue"),
            (Seat::P0, "Test Anthem"),
            (Seat::P0, "Test Spur"),
        ],
    );
    assert_eq!(*s.pending(), Pending::None);
    assert_eq!(s.derived(bear).colors, Colors::BLUE);
    assert_eq!((s.derived(bear).power, s.derived(bear).toughness), (4, 3));
}

/// The active player's permanents take the earlier timestamps, whatever order the batch
/// moves them in: here the opponent's card comes first by name.
#[test]
fn the_active_players_permanents_entering_together_take_the_earlier_timestamps() {
    let (t, mut s, _) = game();
    let [theirs, mine] = enter_together(
        &t,
        &mut s,
        [(Seat::P1, "Test Anthem"), (Seat::P0, "Test Bear")],
    );
    assert_eq!(*s.pending(), Pending::None);
    assert!(s.object(mine).timestamp() < s.object(theirs).timestamp());
}
