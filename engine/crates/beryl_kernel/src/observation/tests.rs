//! Determinization of a shared-deck game: one pool dealt across the shared library and the
//! opponent's hand, keeping every card the viewer knows and the deck list's multiset.

use beryl_card::{CardDef, Seat, Zone};

use crate::bitboard::Bitboard;
use crate::event::Sink;
use crate::format::Format;
use crate::ids::{CardId, SlotId};
use crate::layout::{Large, Layout, Small};
use crate::observation::Observation;
use crate::profile::Profile;
use crate::rng::SplitMix64;
use crate::state::{State, ZoneKey};
use crate::table::{CardTable, GameTable};
use crate::zone::Place;

static ALPHA: CardDef = CardDef {
    name: "Alpha",
    shape: beryl_card::definition::Shape::Single,
    faces: &[],
    tokens: &[],
};
static BETA: CardDef = CardDef {
    name: "Beta",
    shape: beryl_card::definition::Shape::Single,
    faces: &[],
    tokens: &[],
};
static GAMMA: CardDef = CardDef {
    name: "Gamma",
    shape: beryl_card::definition::Shape::Single,
    faces: &[],
    tokens: &[],
};
static DELTA: CardDef = CardDef {
    name: "Delta",
    shape: beryl_card::definition::Shape::Single,
    faces: &[],
    tokens: &[],
};
static CARDS: CardTable = CardTable::new(&[&ALPHA, &BETA, &GAMMA, &DELTA]);

const COUNTS: [u8; 4] = [30, 25, 15, 10];

fn shared_table() -> GameTable {
    GameTable::new(
        Format::SHARED_DECK,
        &CARDS,
        [
            &[
                (COUNTS[0], "Alpha"),
                (COUNTS[1], "Beta"),
                (COUNTS[2], "Gamma"),
                (COUNTS[3], "Delta"),
            ],
            &[],
        ],
    )
    .unwrap()
}

fn multiset<L: Layout>(s: &State<L>, t: &GameTable) -> [u8; 4] {
    let mut counts = [0u8; 4];
    for i in 0..t.deck_len(Seat::P0) {
        let slot = L::Slot::shared_deck(i);
        counts[s.object(slot).card.index()] += 1;
    }
    counts
}

fn check_thousand_determinizations<L: Layout>() {
    let t = shared_table();
    let mut s = State::<L>::deal(&t, 7, Profile::EXACT).unwrap();
    let sink = &mut Sink::off();

    // Both players draw opening cards.
    let mut p0_hand = Vec::new();
    let mut p1_hand = Vec::new();
    for _ in 0..5 {
        p0_hand.push(s.draw(sink, Seat::P0).unwrap().unwrap());
        p1_hand.push(s.draw(sink, Seat::P1).unwrap().unwrap());
    }

    // P0 looks at the top of the shared library and watches P1 draw it.
    let watched_draw = s.library_from_top(Seat::P0).next().unwrap();
    s.look(sink, Seat::P0, Bitboard::single(watched_draw));
    assert_eq!(s.draw(sink, Seat::P1).unwrap(), Some(watched_draw));
    assert!(s.knows(Seat::P0, watched_draw) && s.knows(Seat::P1, watched_draw));

    // P0 puts a known card back on top of the shared library.
    let known_top = p0_hand.pop().unwrap();
    s.move_object(sink, known_top, Zone::Library, Place::Top)
        .unwrap();
    assert!(s.knows(Seat::P0, known_top) && !s.knows(Seat::P1, known_top));

    // Move cards to public zones (shared graveyard and exile).
    let to_grave = p0_hand.pop().unwrap();
    s.move_object(sink, to_grave, Zone::Graveyard, Place::Top)
        .unwrap();
    let to_exile = p1_hand.pop().unwrap();
    s.move_object(sink, to_exile, Zone::Exile, Place::Top)
        .unwrap();
    assert_eq!(s.check_invariants(&t), Ok(()));

    let mut rng = SplitMix64::new(42);
    for viewer in Seat::BOTH {
        let obs = Observation::new(&s, viewer);
        let mut hand_changed = false;
        let mut lib_changed = false;
        let opponent = if viewer == Seat::P0 {
            Seat::P1
        } else {
            Seat::P0
        };
        let opp_hand = s.zone(ZoneKey::Of(opponent, Zone::Hand));
        let library = s.zone(ZoneKey::Of(Seat::P0, Zone::Library));

        for _ in 0..1_000 {
            let det = obs.determinize(&t, &mut rng);
            assert_eq!(det.check_invariants(&t), Ok(()));
            assert_eq!(multiset(&det, &t), COUNTS);

            for i in 0..t.deck_len(Seat::P0) {
                let slot = L::Slot::shared_deck(i);
                if s.knows(viewer, slot) {
                    assert_eq!(det.object(slot).card, s.object(slot).card);
                } else if det.object(slot).card != s.object(slot).card {
                    if opp_hand.contains(slot) {
                        hand_changed = true;
                    }
                    if library.contains(slot) {
                        lib_changed = true;
                    }
                }
            }
            if viewer == Seat::P0 {
                assert_eq!(det.object(known_top).card, s.object(known_top).card);
                assert_eq!(det.object(watched_draw).card, s.object(watched_draw).card);
            }
        }
        assert!(hand_changed, "opponent hand never varied for {viewer:?}");
        assert!(lib_changed, "shared library never varied for {viewer:?}");

        // `determinize_libraries` keeps every hand fixed and redeals unknown library cards.
        let mut lib_only_changed = false;
        for _ in 0..20 {
            let det = obs.determinize_libraries(&t, &mut rng);
            assert_eq!(det.check_invariants(&t), Ok(()));
            assert_eq!(multiset(&det, &t), COUNTS);
            for p in Seat::BOTH {
                for slot in s.zone(ZoneKey::Of(p, Zone::Hand)).iter() {
                    assert_eq!(det.object(slot).card, s.object(slot).card);
                }
            }
            if viewer == Seat::P0 {
                assert_eq!(det.object(known_top).card, s.object(known_top).card);
            }
            lib_only_changed |= library
                .iter()
                .any(|slot| !s.knows(viewer, slot) && det.object(slot).card != s.object(slot).card);
        }
        assert!(lib_only_changed);
    }
}

#[test]
fn a_thousand_determinizations_of_a_shared_position_keep_multiset_and_known_cards() {
    check_thousand_determinizations::<Small>();
    check_thousand_determinizations::<Large>();
}

#[test]
fn a_shared_subgame_determinizes_from_the_shared_deck() {
    let t = shared_table();
    let mut outer = State::<Large>::deal(&t, 11, Profile::EXACT).unwrap();
    let sink = &mut Sink::off();
    for _ in 0..4 {
        outer.draw(sink, Seat::P0).unwrap();
        outer.draw(sink, Seat::P1).unwrap();
    }
    let mut inner = outer.deal_subgame(Format::SHARED_DECK).unwrap();
    let p0_sub = inner.draw(sink, Seat::P0).unwrap().unwrap();
    let p1_sub = inner.draw(sink, Seat::P1).unwrap().unwrap();
    outer.enter_subgame(inner).unwrap();
    let inner = outer;

    let mut rng = SplitMix64::new(99);
    for viewer in Seat::BOTH {
        let det = Observation::new(&inner, viewer).determinize(&t, &mut rng);
        let outer_det = det.suspended_game().expect("suspended shared game");
        assert_eq!(outer_det.check_invariants(&t), Ok(()));
        assert_eq!(multiset(outer_det, &t), COUNTS);
        for slot in [p0_sub, p1_sub] {
            if inner.knows(viewer, slot) {
                assert_eq!(det.object(slot).card, inner.object(slot).card);
            }
        }
        // Every card in the subgame comes from the outer game's shared library multiset.
        let mut inner_counts = [0usize; 4];
        let mut outer_lib_counts = [0usize; 4];
        for i in 0..t.deck_len(Seat::P0) {
            let slot = crate::ids::LargeSlot::shared_deck(i);
            if det.object(slot).zone != Zone::Nowhere {
                inner_counts[det.object(slot).card.index()] += 1;
            }
            if outer_det.object(slot).zone == Zone::Library {
                outer_lib_counts[outer_det.object(slot).card.index()] += 1;
            }
        }
        assert_eq!(inner_counts, outer_lib_counts);
    }
}
