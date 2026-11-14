//! Land types and the mana abilities they give ([CR 305.6], [CR 305.7]), and the order of
//! effects in one layer when one depends on another ([CR 613.8]).
//!
//! [CR 305.6]: https://yawgatog.com/resources/magic-rules/#R3056
//! [CR 305.7]: https://yawgatog.com/resources/magic-rules/#R3057
//! [CR 613.8]: https://yawgatog.com/resources/magic-rules/#R6138

use beryl_card::{AbilityIx, Amount, BasicLandTypes, ManaColor, ManaCost, Objects};

use crate::bitboard::Bitboard;
use crate::card::{Ability, CardDef, Face, LayerSpec, Modification, Subtype, Subtypes, Types};
use crate::event::Sink;
use crate::filter::Filter;
use crate::ids::{Seat, Slot};
use crate::state::{TurnStep, Zone};
use crate::{CardTable, GameTable, Profile, State, layers, reference};

use super::events::find;
use super::{DUAL, LAND};

const fn enchantment(abilities: &'static [Ability]) -> Face {
    Face {
        types: Types::ENCHANTMENT,
        abilities,
        ..Face::EMPTY
    }
}

static NONBASIC_LANDS: Objects = Objects::All {
    zone: Zone::Battlefield,
    filter: Filter::All(&[
        Filter::Types(Types::LAND),
        Filter::Not(&Filter::Supertypes(crate::card::Supertypes::BASIC)),
    ]),
};

/// "Nonbasic lands are Mountains."
static MOON: CardDef = CardDef {
    name: "Test Moon",
    faces: &[enchantment(&[Ability::static_ability(&LayerSpec {
        affected: NONBASIC_LANDS,
        condition: None,
        modifications: &[Modification::SetLandTypes(BasicLandTypes::MOUNTAIN)],
    })])],
    tokens: &[],
};

/// A nonbasic land with no mana ability of its own: "Each land is a Swamp in addition to its
/// other land types."
static TOMB: CardDef = CardDef {
    name: "Test Tomb",
    faces: &[Face {
        types: Types::LAND,
        abilities: &[Ability::static_ability(&LayerSpec {
            affected: Objects::All {
                zone: Zone::Battlefield,
                filter: Filter::Types(Types::LAND),
            },
            condition: None,
            modifications: &[Modification::AddSubtype(Subtype::Swamp)],
        })],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// A nonbasic land whose own ability sets every nonbasic land's type, its own included: two
/// of them remove each other's ability, a loop of dependencies.
static MOONLAND: CardDef = CardDef {
    name: "Test Moonland",
    faces: &[Face {
        types: Types::LAND,
        abilities: &[Ability::static_ability(&LayerSpec {
            affected: NONBASIC_LANDS,
            condition: None,
            modifications: &[Modification::SetLandTypes(BasicLandTypes::ISLAND)],
        })],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// A 1/1 Forest Dryad land creature whose text writes out no mana ability.
static ARBOR: CardDef = CardDef {
    name: "Test Arbor",
    faces: &[Face {
        types: Types::LAND.union(Types::CREATURE),
        subtypes: Subtypes::of(&[Subtype::Forest, Subtype::Dryad]),
        power: Some(1),
        toughness: Some(1),
        ..Face::EMPTY
    }],
    tokens: &[],
};

static ONE: Amount = Amount::Fixed(1);

/// "All creatures lose all abilities and have base power and toughness 1/1."
static HUMBLE: CardDef = CardDef {
    name: "Test Humble",
    faces: &[enchantment(&[Ability::static_ability(&LayerSpec {
        affected: Objects::All {
            zone: Zone::Battlefield,
            filter: Filter::Types(Types::CREATURE),
        },
        condition: None,
        modifications: &[
            Modification::LoseAllAbilities,
            Modification::SetBasePt(&ONE, &ONE),
        ],
    })])],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[&LAND, &DUAL, &MOON, &TOMB, &MOONLAND, &ARBOR, &HUMBLE]);

/// A game in the first player's main phase, each deck holding some of each card.
fn game() -> (GameTable, State) {
    let deck: &[(u8, &str)] = &[
        (4, "Test Land"),
        (2, "Test Dual"),
        (2, "Test Moon"),
        (2, "Test Tomb"),
        (2, "Test Moonland"),
        (2, "Test Arbor"),
        (2, "Test Humble"),
    ];
    let t = GameTable::new(&CARDS, [deck, deck]).unwrap();
    let mut s = State::new(&t, 3, Profile::EXACT).unwrap();
    s.board().begin_at(1, Seat::P0, TurnStep::PrecombatMain);
    (t, s)
}

/// Puts `player`'s `card` onto the battlefield, with a timestamp after every permanent's
/// before it, and refreshes the layers, checking them against a recompute from scratch.
/// Returns its slot and the dependency trials the refresh ran.
fn enters(t: &GameTable, s: &mut State, player: Seat, card: &str) -> (Slot, u64) {
    let slot = find(s, t, player, card);
    s.board().place(slot, Zone::Battlefield).unwrap();
    let mut sink = Sink::off();
    layers::refresh(s, t, &mut sink).unwrap();
    reference::check_references(s, t).unwrap();
    (slot, sink.work.dependency_trials)
}

fn on(t: &GameTable, s: &mut State, card: &str) -> Slot {
    enters(t, s, Seat::P0, card).0
}

fn is(t: &GameTable, s: &State, slot: Slot, subtype: Subtype) -> bool {
    s.derived(slot).subtypes.contains(t.subtypes(), subtype)
}

const U: u8 = 1 << ManaColor::Blue as u8;
const B: u8 = 1 << ManaColor::Black as u8;
const R: u8 = 1 << ManaColor::Red as u8;
const G: u8 = 1 << ManaColor::Green as u8;

/// Whether the first player's untapped lands pay `cost` now.
fn pays(t: &GameTable, s: &State, cost: ManaCost) -> bool {
    crate::mana::payable(s, t, Seat::P0, cost, Bitboard::EMPTY).unwrap()
}

/// A land of a basic land type has that type's mana ability though its text does not write
/// it out ([CR 305.6]): "An object with the land card type and a basic land type has the
/// intrinsic ability “{T}: Add [mana symbol],” even if the text box doesn't actually contain
/// that text or the object has no text box." One that writes it out, as the Plains here
/// does, has it once.
///
/// [CR 305.6]: https://yawgatog.com/resources/magic-rules/#R3056
#[test]
fn a_land_of_a_basic_land_type_has_its_mana_ability_unwritten() {
    let (t, mut s) = game();
    let arbor = on(&t, &mut s, "Test Arbor");
    let plains = on(&t, &mut s, "Test Land");
    assert_eq!(s.intrinsic_mana(&t, arbor), G);
    assert_eq!(s.intrinsic_mana(&t, plains), 0);
    assert!(s.makes_card_mana(arbor));
    assert!(pays(&t, &s, ManaCost::new(0, [1, 0, 0, 0, 1, 0])));
    assert!(!pays(&t, &s, ManaCost::new(0, [0, 0, 0, 0, 2, 0])));
}

/// An effect that sets a nonbasic land's type replaces its land types and the abilities of
/// its text with the new type's mana ability, and leaves a basic land alone ([CR 305.7]):
/// "It loses all abilities generated from its rules text, its old land types, and any
/// copiable effects affecting that land, and it gains the appropriate mana ability for each
/// new basic land type." A land creature stays a creature of its creature types.
///
/// [CR 305.7]: https://yawgatog.com/resources/magic-rules/#R3057
#[test]
fn setting_a_land_type_replaces_its_types_and_mana() {
    let (t, mut s) = game();
    let dual = on(&t, &mut s, "Test Dual");
    let plains = on(&t, &mut s, "Test Land");
    let arbor = on(&t, &mut s, "Test Arbor");
    on(&t, &mut s, "Test Moon");
    assert!(is(&t, &s, dual, Subtype::Mountain));
    assert!(!is(&t, &s, dual, Subtype::Plains) && !is(&t, &s, dual, Subtype::Island));
    assert!(!s.has_ability(dual, AbilityIx::new(0)) && !s.has_ability(dual, AbilityIx::new(1)));
    assert_eq!(s.intrinsic_mana(&t, dual), R);
    assert!(is(&t, &s, plains, Subtype::Plains) && !is(&t, &s, plains, Subtype::Mountain));
    assert!(s.makes_card_mana(plains));
    assert!(is(&t, &s, arbor, Subtype::Dryad) && is(&t, &s, arbor, Subtype::Mountain));
    assert!(!is(&t, &s, arbor, Subtype::Forest));
    assert!(s.derived(arbor).types.intersects(Types::CREATURE));
    assert_eq!(s.intrinsic_mana(&t, arbor), R);
    assert!(pays(&t, &s, ManaCost::new(0, [1, 0, 0, 1, 0, 0])));
    assert!(!pays(&t, &s, ManaCost::new(0, [0, 1, 0, 0, 0, 0])));
}

/// A land that gains a basic land type gains its mana ability and keeps its own
/// ([CR 305.7]): "If a land gains one or more land types in addition to its own, it keeps its
/// land types and rules text, and it gains the new land types and mana abilities." An
/// ability-removing effect in layer 6 then takes the mana abilities of its land types with
/// the rest.
///
/// [CR 305.7]: https://yawgatog.com/resources/magic-rules/#R3057
#[test]
fn a_land_that_gains_a_basic_land_type_gains_its_mana() {
    let (t, mut s) = game();
    let tomb = on(&t, &mut s, "Test Tomb");
    let plains = on(&t, &mut s, "Test Land");
    let arbor = on(&t, &mut s, "Test Arbor");
    assert_eq!(s.intrinsic_mana(&t, tomb), B);
    assert_eq!(s.intrinsic_mana(&t, plains), B);
    assert!(is(&t, &s, plains, Subtype::Plains) && is(&t, &s, plains, Subtype::Swamp));
    assert_eq!(s.intrinsic_mana(&t, arbor), G | B);
    assert!(pays(&t, &s, ManaCost::new(0, [1, 0, 1, 0, 0, 0])));
    on(&t, &mut s, "Test Humble");
    assert_eq!(s.intrinsic_mana(&t, arbor), 0);
    assert_eq!(s.intrinsic_mana(&t, plains), B);
}

/// The Tomb's effect depends on the Moon's, which removes the Tomb's ability, whichever has
/// the earlier timestamp ([CR 613.8a]): "applying the other would change the text or the
/// existence of the first effect". So the Moon's applies first, the Tomb's does not exist,
/// and nothing is a Swamp, as Urborg's ruling has it: "it won't turn lands into Swamps, no
/// matter in what order those effects started to apply."
///
/// [CR 613.8a]: https://yawgatog.com/resources/magic-rules/#R6138a
#[test]
fn an_effect_that_removes_another_s_ability_applies_first_in_either_order() {
    for moon_first in [true, false] {
        let (t, mut s) = game();
        let plains = on(&t, &mut s, "Test Land");
        let (tomb, trials) = if moon_first {
            on(&t, &mut s, "Test Moon");
            enters(&t, &mut s, Seat::P0, "Test Tomb")
        } else {
            let tomb = on(&t, &mut s, "Test Tomb");
            (tomb, enters(&t, &mut s, Seat::P0, "Test Moon").1)
        };
        // With the Moon's the earlier, it depends on nothing and applies first with no
        // trial; with the Tomb's the earlier, the Tomb's is tried and found to wait.
        assert_eq!(trials > 0, !moon_first, "trials: {trials}");
        assert!(
            !is(&t, &s, plains, Subtype::Swamp),
            "moon first: {moon_first}"
        );
        assert!(!is(&t, &s, tomb, Subtype::Swamp));
        assert!(is(&t, &s, tomb, Subtype::Mountain));
        assert_eq!(s.intrinsic_mana(&t, tomb), R);
        assert_eq!(s.intrinsic_mana(&t, plains), 0);
        assert!(!pays(&t, &s, ManaCost::new(0, [0, 0, 1, 0, 0, 0])));
    }
}

/// Two effects that each remove the other's ability form a loop of dependencies, which
/// applies in timestamp order ([CR 613.8b]): "If several dependent effects form a dependency
/// loop, then this rule is ignored and the effects in the dependency loop are applied in
/// timestamp order." The earlier sets the land types and removes the later's ability, so
/// every nonbasic land is an Island and nothing a Mountain or a Swamp.
///
/// [CR 613.8b]: https://yawgatog.com/resources/magic-rules/#R6138b
#[test]
fn a_loop_of_dependencies_applies_in_timestamp_order() {
    let (t, mut s) = game();
    let first = on(&t, &mut s, "Test Moonland");
    let (second, _) = enters(&t, &mut s, Seat::P1, "Test Moonland");
    let dual = on(&t, &mut s, "Test Dual");
    for land in [first, second, dual] {
        assert!(is(&t, &s, land, Subtype::Island));
        assert_eq!(s.intrinsic_mana(&t, land), U);
    }
    assert!(!pays(&t, &s, ManaCost::new(0, [1, 0, 0, 0, 0, 0])));
}

/// Effects in one layer that read nothing the others write are never tried against each
/// other: the Tomb's effect with a resolved one on a land's types alone, and two lands of
/// basic types, need no trial.
#[test]
fn effects_that_read_nothing_the_others_write_are_never_tried() {
    let (t, mut s) = game();
    on(&t, &mut s, "Test Land");
    on(&t, &mut s, "Test Dual");
    let (_, trials) = enters(&t, &mut s, Seat::P0, "Test Tomb");
    assert_eq!(trials, 0);
    let (_, trials) = enters(&t, &mut s, Seat::P1, "Test Tomb");
    assert_eq!(trials, 0);
}
