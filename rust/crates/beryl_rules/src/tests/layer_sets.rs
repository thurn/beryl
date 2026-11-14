//! Static abilities over the permanents a filter accepts, and "as long as" conditions that
//! read the layer results: each is read as its layer applies, against what the layers before
//! it made of every permanent ([CR 613.1]): "all applicable continuous effects are applied in
//! a series of layers in the following order".
//!
//! [CR 613.1]: https://yawgatog.com/resources/magic-rules/#R6131

use beryl_card::{Amount, Comparison, Condition, Objects, Players};

use crate::card::{Ability, CardDef, Face, LayerSpec, Modification, Types};
use crate::filter::Filter;
use crate::ids::{Seat, Slot};
use crate::state::{TurnStep, Zone};
use crate::{CardTable, GameTable, Profile, State, layers, reference};

use super::LAND;
use super::events::find;

/// "Artifacts you control".
static YOUR_ARTIFACTS: Filter =
    Filter::All(&[Filter::Types(Types::ARTIFACT), Filter::Whose(&Players::You)]);

/// "Creatures you control".
static YOUR_CREATURES: Filter =
    Filter::All(&[Filter::Types(Types::CREATURE), Filter::Whose(&Players::You)]);

/// An artifact with no abilities: a 0/0 if it becomes a creature.
static TRINKET: CardDef = CardDef {
    name: "Test Trinket",
    faces: &[Face {
        types: Types::ARTIFACT,
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// A 2/2 creature with mana value 0.
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

/// An enchantment's face with these abilities.
const fn enchantment(abilities: &'static [Ability]) -> Face {
    Face {
        types: Types::ENCHANTMENT,
        abilities,
        ..Face::EMPTY
    }
}

/// "Creatures you control get +1/+1", in layer 7c.
static ANTHEM: CardDef = CardDef {
    name: "Test Anthem",
    faces: &[enchantment(&[Ability::static_ability(&LayerSpec {
        affected: Objects::All {
            zone: Zone::Battlefield,
            filter: YOUR_CREATURES,
        },
        condition: None,
        modifications: &[Modification::ModifyPt(1, 1)],
    })])],
    tokens: &[],
};

/// "Artifacts you control are creatures in addition to their other types", in layer 4.
static ANIMATOR: CardDef = CardDef {
    name: "Test Animator",
    faces: &[enchantment(&[Ability::static_ability(&LayerSpec {
        affected: Objects::All {
            zone: Zone::Battlefield,
            filter: YOUR_ARTIFACTS,
        },
        condition: None,
        modifications: &[Modification::AddTypes(Types::CREATURE)],
    })])],
    tokens: &[],
};

/// "Creatures you control are artifacts in addition to their other types", in layer 4.
static FORGE: CardDef = CardDef {
    name: "Test Forge",
    faces: &[enchantment(&[Ability::static_ability(&LayerSpec {
        affected: Objects::All {
            zone: Zone::Battlefield,
            filter: YOUR_CREATURES,
        },
        condition: None,
        modifications: &[Modification::AddTypes(Types::ARTIFACT)],
    })])],
    tokens: &[],
};

/// "Untapped creatures with mana value 0 get +0/+3", whoever controls them.
static WALL: CardDef = CardDef {
    name: "Test Wall",
    faces: &[enchantment(&[Ability::static_ability(&LayerSpec {
        affected: Objects::All {
            zone: Zone::Battlefield,
            filter: Filter::All(&[
                Filter::Types(Types::CREATURE),
                Filter::Not(&Filter::Tapped),
                Filter::ManaValueAtMost(0),
            ]),
        },
        condition: None,
        modifications: &[Modification::ModifyPt(0, 3)],
    })])],
    tokens: &[],
};

/// "As long as you control two or more creatures, this creature gets +2/+0. As long as it's
/// an artifact, it gets +0/+1": a 0/3.
static CAPTAIN: CardDef = CardDef {
    name: "Test Captain",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(0),
        toughness: Some(3),
        abilities: &[
            Ability::static_ability(&LayerSpec {
                affected: Objects::This,
                condition: Some(Condition::Compare(
                    Amount::Count(&Objects::All {
                        zone: Zone::Battlefield,
                        filter: YOUR_CREATURES,
                    }),
                    Comparison::AtLeast,
                    Amount::Fixed(2),
                )),
                modifications: &[Modification::ModifyPt(2, 0)],
            }),
            Ability::static_ability(&LayerSpec {
                affected: Objects::This,
                condition: Some(Condition::Is(Objects::This, Filter::Types(Types::ARTIFACT))),
                modifications: &[Modification::ModifyPt(0, 1)],
            }),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[
    &LAND, &TRINKET, &BEAR, &ANTHEM, &ANIMATOR, &FORGE, &WALL, &CAPTAIN,
]);

/// A game in the first player's main phase, each deck holding two of each card.
fn game() -> (GameTable, State) {
    let deck: &[(u8, &str)] = &[
        (2, "Test Trinket"),
        (2, "Test Bear"),
        (2, "Test Anthem"),
        (2, "Test Animator"),
        (2, "Test Forge"),
        (2, "Test Wall"),
        (2, "Test Captain"),
        (8, "Test Land"),
    ];
    let t = GameTable::new(&CARDS, [deck, deck]).unwrap();
    let mut s = State::new(&t, 3, Profile::EXACT).unwrap();
    s.board().begin_at(1, Seat::P0, TurnStep::PrecombatMain);
    (t, s)
}

/// Puts `player`'s `card` onto the battlefield and refreshes the layers, checking them
/// against a recompute from scratch.
fn enters(t: &GameTable, s: &mut State, player: Seat, card: &str) -> Slot {
    let slot = find(s, t, player, card);
    s.board().place(slot, Zone::Battlefield).unwrap();
    refresh(t, s);
    slot
}

fn refresh(t: &GameTable, s: &mut State) {
    layers::refresh(s, t, &mut crate::event::Sink::off()).unwrap();
    reference::check_references(s, t).unwrap();
}

fn pt(s: &State, slot: Slot) -> (i8, i8) {
    let d = s.derived(slot);
    (d.power, d.toughness)
}

/// A static ability over a set applies to whatever its filter accepts each time the layers
/// run ([CR 611.3a]): "it applies at any given moment to whatever its text indicates." A
/// creature that enters later gets the bonus, an opponent's creature does not, and two such
/// abilities each apply.
///
/// [CR 611.3a]: https://yawgatog.com/resources/magic-rules/#R6113a
#[test]
fn a_static_ability_applies_to_each_permanent_its_filter_accepts() {
    let (t, mut s) = game();
    let bear = enters(&t, &mut s, Seat::P0, "Test Bear");
    let theirs = enters(&t, &mut s, Seat::P1, "Test Bear");
    enters(&t, &mut s, Seat::P0, "Test Anthem");
    assert_eq!(pt(&s, bear), (3, 3));
    assert_eq!(pt(&s, theirs), (2, 2));
    let late = enters(&t, &mut s, Seat::P0, "Test Bear");
    assert_eq!(pt(&s, late), (3, 3));
    enters(&t, &mut s, Seat::P0, "Test Anthem");
    assert_eq!(pt(&s, bear), (4, 4));
}

/// What an effect applies to is found against the layers before its own
/// ([CR 613.1d], [CR 613.4c]): an artifact a layer 4 effect makes a creature gets a layer 7c
/// bonus for creatures, whichever of the two effects began first.
///
/// [CR 613.1d]: https://yawgatog.com/resources/magic-rules/#R6131d
/// [CR 613.4c]: https://yawgatog.com/resources/magic-rules/#R6134c
#[test]
fn a_set_is_found_against_the_layers_before_its_own() {
    let (t, mut s) = game();
    enters(&t, &mut s, Seat::P0, "Test Anthem");
    let trinket = enters(&t, &mut s, Seat::P0, "Test Trinket");
    assert!(!s.derived(trinket).types.intersects(Types::CREATURE));
    enters(&t, &mut s, Seat::P0, "Test Animator");
    assert!(s.derived(trinket).types.intersects(Types::CREATURE));
    assert_eq!(pt(&s, trinket), (1, 1));
}

/// A filter may ask whether a permanent is tapped and what its mana value is, which no layer
/// changes, and is read for the controller of the permanent with the ability.
#[test]
fn a_filter_reads_whether_a_permanent_is_tapped_and_its_mana_value() {
    let (t, mut s) = game();
    let bear = enters(&t, &mut s, Seat::P0, "Test Bear");
    let theirs = enters(&t, &mut s, Seat::P1, "Test Bear");
    enters(&t, &mut s, Seat::P0, "Test Wall");
    assert_eq!(pt(&s, bear), (2, 5));
    assert_eq!(pt(&s, theirs), (2, 5));
    s.board().set_tapped(bear, true);
    s.forget_derived();
    refresh(&t, &mut s);
    assert_eq!(pt(&s, bear), (2, 2));
    assert_eq!(pt(&s, theirs), (2, 5));
}

/// "As long as you control two or more creatures" counts the creatures as the layers before
/// left them, and "as long as it's an artifact" reads its types the same way: a layer 4
/// effect that makes artifacts creatures adds to the count, and one that makes creatures
/// artifacts makes it an artifact.
#[test]
fn a_condition_on_the_layer_results_is_read_as_its_layer_applies() {
    let (t, mut s) = game();
    let captain = enters(&t, &mut s, Seat::P0, "Test Captain");
    assert_eq!(pt(&s, captain), (0, 3));
    enters(&t, &mut s, Seat::P0, "Test Trinket");
    assert_eq!(pt(&s, captain), (0, 3));
    enters(&t, &mut s, Seat::P0, "Test Animator");
    assert_eq!(pt(&s, captain), (2, 3));
    enters(&t, &mut s, Seat::P0, "Test Forge");
    assert_eq!(pt(&s, captain), (2, 4));
    // The opponent's creatures are not "you control".
    let (t, mut s) = game();
    let captain = enters(&t, &mut s, Seat::P0, "Test Captain");
    enters(&t, &mut s, Seat::P1, "Test Bear");
    assert_eq!(pt(&s, captain), (0, 3));
    enters(&t, &mut s, Seat::P0, "Test Bear");
    assert_eq!(pt(&s, captain), (2, 3));
}
