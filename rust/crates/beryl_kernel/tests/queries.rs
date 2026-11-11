//! The kernel's queries (`beryl_kernel::query`), each asked of a card, a token, a copy, a
//! face-down permanent and a double-faced one.

use beryl_card::{
    CardDef, FACE_DOWN, Face, FaceIx, ManaCost, Types,
    Zone::{self, Battlefield, Graveyard, Hand, Nowhere, Stack},
};
use beryl_kernel::event::Sink;
use beryl_kernel::mutate::Arrival;
use beryl_kernel::profile::Profile;
use beryl_kernel::state::{Derived, StackEntry};
use beryl_kernel::table::CardTable;
use beryl_kernel::zone::Place;
use beryl_kernel::{Copiable, GameTable, Observation, Seat, Slot, SlotId, State};

static SOLDIER: CardDef = CardDef {
    name: "Test Soldier",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        ..Face::EMPTY
    }],
    tokens: &[],
};

static BEAR: CardDef = CardDef {
    name: "Test Bear",
    faces: &[Face {
        cost: ManaCost::new(1, [0, 0, 0, 0, 1, 0]),
        types: Types::CREATURE,
        power: Some(2),
        toughness: Some(3),
        ..Face::EMPTY
    }],
    tokens: &[&SOLDIER],
};

/// A transforming double-faced card: a creature whose back face is a planeswalker.
static PRODIGY: CardDef = CardDef {
    name: "Test Prodigy",
    faces: &[
        Face {
            cost: ManaCost::new(1, [0, 1, 0, 0, 0, 0]),
            types: Types::CREATURE,
            power: Some(0),
            toughness: Some(2),
            ..Face::EMPTY
        },
        Face {
            name: "Test Telepath",
            types: Types::PLANESWALKER,
            loyalty: Some(5),
            transformed: true,
            ..Face::EMPTY
        },
    ],
    tokens: &[],
};

/// A sorcery that costs {X}{R}.
static BLAST: CardDef = CardDef {
    name: "Test Blast",
    faces: &[Face {
        cost: ManaCost {
            generic: 0,
            symbols: [0, 0, 0, 1, 0, 0],
            x: 1,
        },
        types: Types::SORCERY,
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[&BEAR, &PRODIGY, &BLAST]);

fn game() -> (GameTable, State) {
    let t = GameTable::new(
        &CARDS,
        [
            &[(2, "Test Bear"), (1, "Test Prodigy"), (1, "Test Blast")],
            &[(2, "Test Bear")],
        ],
    )
    .unwrap();
    let s = State::new(&t, 7, Profile::EXACT).unwrap();
    (t, s)
}

/// A card named `name` in `player`'s library, moved to `zone`.
fn put(t: &GameTable, s: &mut State, player: Seat, name: &str, zone: Zone) -> Slot {
    let slot = (s.library(player).iter().copied())
        .find(|&slot| s.definition_name(t, slot) == name)
        .unwrap_or_else(|| panic!("no {name} in the library"));
    s.board().place(slot, zone).unwrap();
    slot
}

/// A token with the copiable values `what`, created by `owner`.
fn token(t: &GameTable, s: &mut State, owner: Seat, what: Copiable) -> Slot {
    let slot = Slot::pool()
        .find(|&slot| s.object(slot).zone() == Nowhere)
        .unwrap();
    let face = s.make_token(t, slot, owner, what);
    s.move_object(&mut Sink::off(), slot, Battlefield, Place::Top)
        .unwrap();
    s.arrive(
        slot,
        Arrival {
            tapped: false,
            attacking: false,
            controller: None,
            face,
            counters: None,
            attach: None,
        },
    );
    slot
}

/// A token copy of the permanent in `of`, 1/1 by exception if `one_one` (CR 707.9b).
fn copy_of(t: &GameTable, s: &mut State, owner: Seat, of: Slot, one_one: bool) -> Slot {
    let what = s.copiable(of);
    let what = if one_one { what.except_one_one() } else { what };
    token(t, s, owner, what)
}

/// Stores layer results with no continuous effect but `boost`, which adds to every power.
fn derive(t: &GameTable, s: &mut State, boost: i8) {
    let Some(mut cache) = s.rederive() else {
        return;
    };
    for i in 0..cache.battlefield().len() {
        let slot = cache.battlefield()[i];
        let what = cache.copiable(slot);
        let controller = cache.base_controller(slot);
        cache.results()[i] = Derived {
            power: what.power(t) + boost,
            toughness: what.toughness(t),
            types: what.face(t).types,
            controller,
            ..Derived::default()
        };
    }
    cache.finish(0);
}

#[test]
fn only_a_card_is_behind_an_object() {
    let (t, mut s) = game();
    let bear = put(&t, &mut s, Seat::P0, "Test Bear", Battlefield);
    let in_library = s.library(Seat::P1)[0];
    let index = t.index_of(&BEAR);
    assert_eq!(s.card_behind(bear), index);
    assert_eq!(s.card_behind(in_library), index);

    // CR 111.1, 111.6: a predefined token and a token copy of a card are no cards.
    let soldier = Copiable::of_card(t.index_of(&SOLDIER).unwrap());
    let soldier = token(&t, &mut s, Seat::P0, soldier);
    let copy = copy_of(&t, &mut s, Seat::P1, bear, false);
    assert_eq!(s.card_behind(soldier), None);
    assert_eq!(s.card_behind(copy), None);
    assert_eq!(s.definition_name(&t, copy), "Test Bear");
    // CR 111.7: it stays a token wherever it goes.
    s.move_object(&mut Sink::off(), copy, Graveyard, Place::Top)
        .unwrap();
    assert_eq!(s.card_behind(copy), None);

    // A face-down permanent's is the rules' own answer; its opponent's view has none.
    s.board().set_face(bear, FaceIx::DOWN);
    assert_eq!(s.card_behind(bear), index);
    assert_eq!(Observation::new(&s, Seat::P0).card(bear), index);
    assert_eq!(Observation::new(&s, Seat::P1).card(bear), None);
}

#[test]
fn copiable_values_are_the_face_up_or_the_blank_face_down_ones() {
    let (t, mut s) = game();
    let bear = put(&t, &mut s, Seat::P0, "Test Bear", Battlefield);
    let prodigy = put(&t, &mut s, Seat::P0, "Test Prodigy", Battlefield);
    let bear_index = t.index_of(&BEAR).unwrap();

    let values = s.copiable(bear);
    assert_eq!(values.name(&t), "Test Bear");
    assert_eq!((values.power(&t), values.toughness(&t)), (2, 3));
    assert_eq!(values.record(&t), (bear_index, FaceIx::FRONT));
    assert_eq!(values, Copiable::of_card(bear_index));

    // CR 712.6d: with its back face up, a double-faced permanent has only that face's.
    s.board().set_face(prodigy, FaceIx::BACK);
    let back = s.copiable(prodigy);
    assert_eq!(back.name(&t), "Test Telepath");
    assert_eq!(back.face(&t).loyalty, Some(5));
    assert_eq!(
        back.record(&t),
        (t.index_of(&PRODIGY).unwrap(), FaceIx::BACK)
    );
    assert_eq!(back.turned(FaceIx::FRONT).name(&t), "Test Prodigy");

    // CR 708.2: a face-down permanent's are the blank 2/2's, and say nothing of its card.
    s.board().set_face(bear, FaceIx::DOWN);
    let down = s.copiable(bear);
    assert!(std::ptr::eq(down.face(&t), &raw const FACE_DOWN));
    assert_eq!(down.name(&t), "");
    assert_eq!((down.power(&t), down.toughness(&t)), (2, 2));
    assert_eq!(down.may_listen(&t), 0);
    assert_eq!(
        down.record(&t),
        (t.face_down_copy::<beryl_kernel::Small>(), FaceIx::FRONT)
    );
}

#[test]
fn a_copy_has_the_copiable_values_and_never_a_hidden_card() {
    let (t, mut s) = game();
    let bear = put(&t, &mut s, Seat::P0, "Test Bear", Battlefield);
    let prodigy = put(&t, &mut s, Seat::P0, "Test Prodigy", Battlefield);
    s.board().set_face(prodigy, FaceIx::BACK);

    // CR 707.2: the copy has what the original has.
    let copy = copy_of(&t, &mut s, Seat::P1, bear, false);
    assert_eq!(s.copiable(copy), s.copiable(bear));
    assert_eq!(s.object(copy).owner(), Seat::P1);

    // CR 707.9b: an exception is part of the copy's own copiable values, so a copy of the
    // copy has it too.
    let small = copy_of(&t, &mut s, Seat::P1, bear, true);
    let smaller = copy_of(&t, &mut s, Seat::P0, small, false);
    for slot in [small, smaller] {
        let values = s.copiable(slot);
        assert_eq!((values.power(&t), values.toughness(&t)), (1, 1));
        assert_eq!(values.name(&t), "Test Bear");
    }

    // CR 707.8a: a copy of a double-faced permanent has the same face up.
    let telepath = copy_of(&t, &mut s, Seat::P1, prodigy, false);
    assert_eq!(s.object(telepath).face(), FaceIx::BACK);
    assert_eq!(s.copiable(telepath).name(&t), "Test Telepath");

    // CR 707.2, 708.2: a copy of a face-down permanent is a face-up blank 2/2, with nothing
    // of the card behind the original.
    s.board().set_face(bear, FaceIx::DOWN);
    let blank = copy_of(&t, &mut s, Seat::P1, bear, false);
    assert_eq!(s.object(blank).face(), FaceIx::FRONT);
    assert_eq!(
        s.copiable(blank).record(&t),
        (t.face_down_copy::<beryl_kernel::Small>(), FaceIx::FRONT)
    );
    assert_ne!(s.copiable(blank).key(), s.copiable(bear).key());
    assert_eq!(s.definition_name(&t, blank), "");
    assert!(s.knows(Seat::P0, blank) && s.knows(Seat::P1, blank));
}

#[test]
fn mana_value_counts_x_on_the_stack_and_a_copied_back_face_as_nothing() {
    let (t, mut s) = game();
    let bear = put(&t, &mut s, Seat::P0, "Test Bear", Battlefield);
    let prodigy = put(&t, &mut s, Seat::P0, "Test Prodigy", Battlefield);
    assert_eq!(s.mana_value(&t, bear), 2);
    assert_eq!(s.mana_value(&t, prodigy), 2);

    // CR 712.6d: a transformed permanent's is its front face's, and a copy of its back
    // face's is 0.
    s.board().set_face(prodigy, FaceIx::BACK);
    assert_eq!(s.mana_value(&t, prodigy), 2);
    let telepath = copy_of(&t, &mut s, Seat::P1, prodigy, false);
    assert_eq!(s.mana_value(&t, telepath), 0);

    // A token copy of a card has the card's mana cost (CR 707.2).
    let copy = copy_of(&t, &mut s, Seat::P1, bear, false);
    assert_eq!(s.mana_value(&t, copy), 2);

    // CR 708.2a: a face-down permanent has no mana cost.
    s.board().set_face(bear, FaceIx::DOWN);
    assert_eq!(s.mana_value(&t, bear), 0);

    // CR 202.3e: X is 0 everywhere but on the stack, where it is the number chosen.
    let blast = put(&t, &mut s, Seat::P0, "Test Blast", Hand);
    assert_eq!(s.mana_value(&t, blast), 1);
    s.board().place(blast, Stack).unwrap();
    s.become_spell(blast, FaceIx::FRONT, Seat::P0);
    let (card, face) = s.copiable(blast).record(&t);
    let entry = StackEntry {
        object: Some(s.object_ref(blast)),
        controller: Seat::P0,
        card,
        face,
        x: 3,
        ..StackEntry::default()
    };
    s.put_on_stack(entry).unwrap();
    assert_eq!(s.mana_value(&t, blast), 4);
}

#[test]
fn a_controller_is_a_permanents_a_spells_or_else_the_owner() {
    let (t, mut s) = game();
    // CR 108.4a: a card that is no permanent or spell has no controller; its owner is used.
    let in_library = s.library(Seat::P1)[0];
    assert_eq!(s.controller(in_library), Seat::P1);

    // A permanent's is the layers' result.
    let bear = put(&t, &mut s, Seat::P0, "Test Bear", Battlefield);
    s.board().set_controller(bear, Seat::P1);
    derive(&t, &mut s, 0);
    assert_eq!(s.controller(bear), Seat::P1);
    assert_eq!(s.object(bear).owner(), Seat::P0);

    // CR 111.2: a token's creator owns it, and controls it as it enters.
    let copy = copy_of(&t, &mut s, Seat::P1, bear, false);
    derive(&t, &mut s, 0);
    assert_eq!(s.controller(copy), Seat::P1);

    // CR 601.2a: a spell's is the player who cast it, whoever owns the card.
    let blast = put(&t, &mut s, Seat::P0, "Test Blast", Stack);
    assert_eq!(s.controller(blast), Seat::P0);
    s.become_spell(blast, FaceIx::FRONT, Seat::P1);
    assert_eq!(s.controller(blast), Seat::P1);
    // And once it has left the stack, its owner again.
    s.board().place(blast, Graveyard).unwrap();
    assert_eq!(s.controller(blast), Seat::P0);
    s.board().place(bear, Graveyard).unwrap();
    assert_eq!(s.controller(bear), Seat::P0);
}

#[test]
fn power_and_toughness_are_the_layers_on_the_battlefield_and_copiable_elsewhere() {
    let (t, mut s) = game();
    let bear = put(&t, &mut s, Seat::P0, "Test Bear", Battlefield);
    let prodigy = put(&t, &mut s, Seat::P0, "Test Prodigy", Battlefield);
    s.board().set_face(prodigy, FaceIx::BACK);
    let small = copy_of(&t, &mut s, Seat::P1, bear, true);
    let in_library = s.library(Seat::P1)[0];
    derive(&t, &mut s, 4);
    assert_eq!((s.power(&t, bear), s.toughness(&t, bear)), (6, 3));
    assert_eq!((s.power(&t, small), s.toughness(&t, small)), (5, 1));
    // A face with no power or toughness has 0 to add to.
    assert_eq!((s.power(&t, prodigy), s.toughness(&t, prodigy)), (4, 0));
    s.board().set_face(bear, FaceIx::DOWN);
    derive(&t, &mut s, 4);
    assert_eq!((s.power(&t, bear), s.toughness(&t, bear)), (6, 2));

    // Off the battlefield they are the copiable values: its card's, or for a token those
    // its creation gave it (CR 111.3, 707.9b), until it ceases to exist.
    assert_eq!(
        (s.power(&t, in_library), s.toughness(&t, in_library)),
        (2, 3)
    );
    for slot in [bear, prodigy, small] {
        s.move_object(&mut Sink::off(), slot, Graveyard, Place::Top)
            .unwrap();
    }
    assert_eq!((s.power(&t, bear), s.toughness(&t, bear)), (2, 3));
    assert_eq!((s.power(&t, prodigy), s.toughness(&t, prodigy)), (0, 2));
    assert_eq!((s.power(&t, small), s.toughness(&t, small)), (1, 1));
}
