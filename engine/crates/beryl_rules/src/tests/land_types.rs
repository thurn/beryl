//! Land types and the mana abilities they give ([CR 305.6], [CR 305.7]), and the order of
//! effects in one layer when one depends on another ([CR 613.8]).
//!
//! [CR 305.6]: https://yawgatog.com/resources/magic-rules/#R3056
//! [CR 305.7]: https://yawgatog.com/resources/magic-rules/#R3057
//! [CR 613.8]: https://yawgatog.com/resources/magic-rules/#R6138

use beryl_card::{
    AbilityIx, Amount, BasicLandTypes, CopyChange, CopyException, ExceptIx, FaceIx, ManaColor,
    ManaCost, Objects, SubtypeWord, TextWord,
};

use crate::additions::Addition;
use crate::bitboard::Bitboard;
use crate::card::{Ability, CardDef, Face, LayerSpec, Modification, Subtype, Subtypes, Types};
use crate::filter::Filter;
use crate::ids::{Seat, Slot};
use crate::replacement::event::Sink;
use crate::replacement::event::TokenEntry;
use crate::state::{
    AbilityRef, Continuous, Duration, LayerChange, StackEntry, StackItem, TriggerInfo, TurnStep,
    Zone,
};
use crate::{CardTable, GameTable, Profile, State, continuous::layers, reference};
use crate::{Scratch, zones::tokens};
use beryl_kernel::format::Format;

use super::events::{find, with_game};
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
    shape: beryl_card::definition::Shape::Single,
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
    shape: beryl_card::definition::Shape::Single,
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
    shape: beryl_card::definition::Shape::Single,
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
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::LAND.union(Types::CREATURE),
        subtypes: crate::card::SubtypeWords::new(Subtypes::of(&[Subtype::Forest, Subtype::Dryad])),
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
    shape: beryl_card::definition::Shape::Single,
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

/// "Each Forest is an artifact in addition to its other types": a set that a land type change
/// in the same layer can empty, as Life and Limb's "All Forests" can be.
static GROVE: CardDef = CardDef {
    name: "Test Grove",
    shape: beryl_card::definition::Shape::Single,
    faces: &[enchantment(&[Ability::static_ability(&LayerSpec {
        affected: Objects::All {
            zone: Zone::Battlefield,
            filter: Filter::Subtype(Subtype::Forest),
        },
        condition: None,
        modifications: &[Modification::AddTypes(Types::ARTIFACT)],
    })])],
    tokens: &[],
};

/// Phantasmal Image's exception: its copy is "an Illusion in addition to its other types".
static MIMIC: CardDef = CardDef {
    name: "Test Mimic",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::CREATURE,
        excepts: &[CopyException(&[CopyChange::AddSubtype(Subtype::Illusion)])],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[
    &LAND, &DUAL, &MOON, &TOMB, &MOONLAND, &ARBOR, &HUMBLE, &GROVE, &MIMIC,
]);

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
        (2, "Test Grove"),
        (2, "Test Mimic"),
    ];
    let t = GameTable::new(Format::CONSTRUCTED, &CARDS, [deck, deck]).unwrap();
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
    crate::mana::payable(
        s,
        t,
        Seat::P0,
        cost,
        Bitboard::EMPTY,
        crate::mana::Paying::NOTHING,
    )
    .unwrap()
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
    assert!(s.makes_card_mana(&t, arbor));
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
    assert!(s.makes_card_mana(&t, plains));
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

#[test]
fn large_layer_orders_dependencies_above_the_byte_limit() {
    let (t, small) = game();
    let mut s = small.widen();
    let [plains, tomb, moon] =
        ["Test Land", "Test Tomb", "Test Moon"].map(|name| find(&s, &t, Seat::P0, name));
    s.board().place(plains, Zone::Battlefield).unwrap();
    s.board().place(tomb, Zone::Battlefield).unwrap();

    // These resolved effects put the Moon's later than a byte can index in layer 4.
    let source = s.card_behind(tomb).unwrap();
    let change = Continuous::Layer(
        s.permanent_at(plains).unwrap(),
        LayerChange::AddTypes(Types::NONE),
    );
    for _ in 0..256 {
        s.start_effect(source, change, Duration::Indefinite)
            .unwrap();
    }
    s.board().place(moon, Zone::Battlefield).unwrap();
    let mut sink = Sink::off();
    layers::refresh(&mut s, &t, &mut sink).unwrap();
    assert!(sink.work.dependency_trials > 0);
    assert!(
        s.derived(tomb)
            .subtypes
            .contains(t.subtypes(), Subtype::Mountain)
    );
    assert!(
        !s.derived(plains)
            .subtypes
            .contains(t.subtypes(), Subtype::Swamp)
    );
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

/// An effect whose set another effect of its layer empties depends on it ([CR 613.8a]):
/// "applying the other would change ... what it applies to". The Moon makes Dryad Arbor a
/// Mountain and no Forest, so the Grove's effect, whichever is earlier, waits for the Moon's
/// and finds no Forest: the Arbor is no artifact. In timestamp order alone, a Grove before the
/// Moon would have made it one.
///
/// [CR 613.8a]: https://yawgatog.com/resources/magic-rules/#R6138a
#[test]
fn an_effect_whose_set_a_type_change_empties_waits_for_it() {
    for grove_first in [true, false] {
        let (t, mut s) = game();
        let arbor = on(&t, &mut s, "Test Arbor");
        let first = if grove_first {
            "Test Grove"
        } else {
            "Test Moon"
        };
        let second = if grove_first {
            "Test Moon"
        } else {
            "Test Grove"
        };
        on(&t, &mut s, first);
        let (_, trials) = enters(&t, &mut s, Seat::P1, second);
        assert!(trials > 0);
        assert!(is(&t, &s, arbor, Subtype::Mountain));
        assert!(
            !s.derived(arbor).types.intersects(Types::ARTIFACT),
            "grove first: {grove_first}"
        );
    }
}

/// A text change on a copy changes the type line it copied, and keeps the subtype its copy
/// exception adds ([CR 707.9b]): "Some copy effects modify a characteristic as part of the
/// copying process." A copy of Test Arbor that is also an Illusion, whose "Forest" reads
/// "Island", is an Island Dryad Illusion.
///
/// [CR 707.9b]: https://yawgatog.com/resources/magic-rules/#R7079b
#[test]
fn a_text_change_on_a_copy_keeps_the_subtype_its_exception_adds() {
    let (t, mut s) = game();
    let mut x = Scratch::new();
    let arbor = on(&t, &mut s, "Test Arbor");
    let illusion = Addition::new(
        t.index_named("Test Mimic").unwrap(),
        FaceIx::FRONT,
        ExceptIx::new(0),
    );
    let values = s.copiable(arbor).with_addition(illusion).unwrap();
    let mut made = Bitboard::EMPTY;
    with_game(&t, &mut s, &mut x, arbor, |game, resolution| {
        made = tokens::create(
            game,
            resolution.controller(),
            values,
            1,
            TokenEntry::default(),
        )
        .unwrap();
    });
    let copy = made.sole().unwrap();
    assert!(is(&t, &s, copy, Subtype::Illusion) && is(&t, &s, copy, Subtype::Forest));
    let card = s.copiable(arbor).record(&t).0;
    let change = TextWord::LandType(Subtype::Forest)
        .to(TextWord::LandType(Subtype::Island))
        .unwrap();
    let effect = Continuous::Text(s.object_ref(copy), change);
    s.start_effect(card, effect, Duration::Indefinite).unwrap();
    layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();
    assert!(is(&t, &s, copy, Subtype::Island) && !is(&t, &s, copy, Subtype::Forest));
    assert!(is(&t, &s, copy, Subtype::Dryad));
    assert!(
        is(&t, &s, copy, Subtype::Illusion),
        "the exception's subtype stays"
    );
}

/// Text changes compose in timestamp order (CR 613.7), and an ability on the stack keeps
/// the words its source had when it went there (CR 113.7a).
#[test]
fn five_land_word_changes_compose_in_live_and_captured_text() {
    let (t, mut s) = game();
    let mut sink = Sink::off();
    let land = on(&t, &mut s, "Test Dual");
    let source = s.object_ref(land);
    let card = s.copiable(land).record(&t).0;
    let ability = || {
        StackEntry::new(StackItem::Triggered {
            of: AbilityRef {
                source: Some(source),
                card,
                controller: Seat::P0.into(),
                ..AbilityRef::default()
            },
            info: TriggerInfo::default(),
        })
    };
    let changes = [
        (Subtype::Island, Subtype::Swamp),
        (Subtype::Plains, Subtype::Island),
        (Subtype::Swamp, Subtype::Mountain),
        (Subtype::Mountain, Subtype::Forest),
        (Subtype::Forest, Subtype::Swamp),
    ];
    for (index, (from, to)) in changes.into_iter().enumerate() {
        let change = TextWord::LandType(from).to(TextWord::LandType(to)).unwrap();
        s.start_effect(card, Continuous::Text(source, change), Duration::Indefinite)
            .unwrap();
        // Capture the source's words just before and just after the fifth change.
        if index >= 3 {
            s.put_on_stack(&mut sink, ability()).unwrap();
        }
    }
    layers::refresh(&mut s, &t, &mut sink).unwrap();

    assert!(is(&t, &s, land, Subtype::Swamp));
    assert!(is(&t, &s, land, Subtype::Island));
    assert!(!is(&t, &s, land, Subtype::Forest));
    let island = SubtypeWord::new(Subtype::Island);
    let plains = SubtypeWord::new(Subtype::Plains);
    let before = s.entry_words(&s.stack()[0]);
    let after = s.entry_words(&s.stack()[1]);
    assert_eq!(island.read(&before), Subtype::Forest);
    assert_eq!(island.read(&after), Subtype::Swamp);
    assert_eq!(plains.read(&before), Subtype::Island);
    assert_eq!(plains.read(&after), Subtype::Island);
}
