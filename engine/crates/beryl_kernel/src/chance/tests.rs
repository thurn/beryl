//! Recording and replaying chance in a shared-deck game: one reveal per card in the shared
//! library, taker as owner on draws, seat 0 on in-place reveals, and no swaps crossing from
//! the library into a hand.

use beryl_card::{CardDef, Seat, Zone};

use crate::bitboard::Bitboard;
use crate::chance::{Chance, Random, adopt_unknown};
use crate::event::Sink;
use crate::format::Format;
use crate::ids::CardId;
use crate::layout::Small;
use crate::profile::Profile;
use crate::state::State;
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

fn shared_table() -> GameTable {
    GameTable::new(
        Format::SHARED_DECK,
        &CARDS,
        [
            &[(30, "Alpha"), (25, "Beta"), (24, "Gamma"), (1, "Delta")],
            &[],
        ],
    )
    .unwrap()
}

fn sink_with(chance: Chance) -> Sink<Small> {
    let mut sink = Sink::off();
    sink.set_chance(Some(chance));
    sink
}

#[test]
fn settle_all_records_one_reveal_per_card_in_a_shared_library() {
    let t = shared_table();
    let mut s = State::<Small>::deal(&t, 7, Profile::EXACT).unwrap();
    let mut sink = sink_with(Chance::default());

    let top_three: Vec<_> = s.library_from_top(Seat::P0).take(3).collect();
    let cards: Bitboard<_> = top_three.iter().copied().collect();
    s.look(&mut sink, Seat::P1, cards);

    let mut chance = sink.take_chance().unwrap();
    let reveals = chance.take();
    assert_eq!(reveals.len(), 3);
    for (reveal, slot) in reveals.into_iter().zip(top_three) {
        assert_eq!(
            reveal,
            Random::Reveal {
                owner: Seat::P0,
                card: s.object(slot).card.wide(),
            }
        );
    }
}

#[test]
fn draw_from_shared_library_records_the_drawer_as_owner() {
    let t = shared_table();
    let mut s = State::<Small>::deal(&t, 7, Profile::EXACT).unwrap();
    let mut sink = sink_with(Chance::default());

    let p1_card = s.draw(&mut sink, Seat::P1).unwrap().unwrap();
    let p0_card = s.draw(&mut sink, Seat::P0).unwrap().unwrap();

    let mut chance = sink.take_chance().unwrap();
    assert_eq!(
        chance.take(),
        vec![
            Random::Reveal {
                owner: Seat::P1,
                card: s.object(p1_card).card.wide(),
            },
            Random::Reveal {
                owner: Seat::P0,
                card: s.object(p0_card).card.wide(),
            },
        ]
    );
}

#[test]
fn partner_never_swaps_a_shared_library_card_with_a_hand() {
    let t = shared_table();
    let mut s = State::<Small>::deal(&t, 7, Profile::EXACT).unwrap();
    let no_chance = &mut Sink::off();

    // Put the single copy of Delta in P1's hand and make it unknown to both players.
    let delta_card = t
        .deck(Seat::P0)
        .find(|&c| t.def(c).name == "Delta")
        .unwrap();
    let delta_slot = s
        .library(Seat::P0)
        .iter()
        .copied()
        .find(|&slot| s.object(slot).card == delta_card)
        .unwrap();
    s.claim(delta_slot, Seat::P1);
    s.move_object(no_chance, delta_slot, Zone::Hand, Place::Top)
        .unwrap();
    s.forget(Seat::P1, delta_slot);
    assert!(!s.known_to_someone().contains(delta_slot));

    // Replaying a reveal of Delta on top of the shared library must mismatch instead of
    // swapping with the unknown card in P1's hand.
    let mut chance = Chance::default();
    chance.replay([Random::Reveal {
        owner: Seat::P0,
        card: delta_card.wide(),
    }]);
    let mut sink = sink_with(chance);
    let top = s.library_from_top(Seat::P0).next().unwrap();
    s.look(&mut sink, Seat::P0, Bitboard::single(top));

    let mut chance = sink.take_chance().unwrap();
    assert!(chance.take_mismatch().is_some());
    assert_eq!(s.object(delta_slot).card, delta_card);
}

#[test]
fn a_recorded_shared_sequence_replays_from_another_seed_without_mismatch() {
    let t = shared_table();

    let run = |seed: u64, record: Option<Vec<Random>>| {
        let mut s = State::<Small>::deal(&t, seed, Profile::EXACT).unwrap();
        let mut chance = Chance::default();
        if let Some(rec) = record {
            chance.replay(rec);
        }
        let mut sink = sink_with(chance);

        let mut seen = Vec::new();
        let d1 = s.draw(&mut sink, Seat::P0).unwrap().unwrap();
        seen.push(s.object(d1).card);
        let d2 = s.draw(&mut sink, Seat::P1).unwrap().unwrap();
        seen.push(s.object(d2).card);

        let top3: Vec<_> = s.library_from_top(Seat::P0).take(3).collect();
        s.look(&mut sink, Seat::P1, top3.iter().copied().collect());
        for &slot in &top3 {
            seen.push(s.object(slot).card);
        }

        // Adopt unknown into a copy of the state before shuffling, mirroring paused resolution.
        let mut scratch = s;
        let d3 = scratch.draw(&mut sink, Seat::P0).unwrap().unwrap();
        seen.push(scratch.object(d3).card);
        adopt_unknown(&mut s, &scratch);
        s = scratch;

        s.move_object(&mut sink, d1, Zone::Graveyard, Place::Top)
            .unwrap();
        s.shuffle_library(Seat::P0).unwrap();

        let top = s.library_from_top(Seat::P0).next().unwrap();
        s.reveal(&mut sink, Seat::P0, top);
        seen.push(s.object(top).card);
        let d4 = s.draw(&mut sink, Seat::P1).unwrap().unwrap();
        seen.push(s.object(d4).card);

        assert_eq!(s.check_invariants(&t), Ok(()));
        let mut chance = sink.take_chance().unwrap();
        (seen, chance.take(), chance.take_mismatch())
    };

    let (seen_a, record, mismatch_a) = run(1, None);
    assert_eq!(mismatch_a, None);
    assert!(!record.is_empty());

    let (seen_b, replayed, mismatch_b) = run(999, Some(record.clone()));
    assert_eq!(mismatch_b, None);
    assert_eq!(seen_b, seen_a);
    assert_eq!(replayed, record);
}
