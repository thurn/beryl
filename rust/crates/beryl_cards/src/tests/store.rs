//! The cards complete from the card store, which have no file: cards like any other, found by
//! name and played from a decklist, that no sweep over every card walks.

use beryl_card::Keywords;
use beryl_engine::{GameTable, State};

use crate::library::keywords::evasion::{FLYER, VANILLA};
use crate::{CARDS, SCENARIOS};

#[test]
fn a_store_card_is_found_by_name_with_its_printed_face_and_keywords() {
    let flyer = CARDS.get(FLYER).expect("the flyer is a card");
    let face = flyer.front();
    assert_eq!((face.power, face.toughness), (Some(4), Some(4)));
    assert_eq!(face.keywords, Keywords::FLYING.union(Keywords::VIGILANCE));
    assert!(face.abilities.is_empty());
    let vanilla = CARDS.get(VANILLA).expect("the vanilla creature is a card");
    assert_eq!(vanilla.front().keywords, Keywords::NONE);
}

/// The two lists hold different cards, and only the cards with files have scenarios of their
/// own: the sweeps that walk `CARDS.cards()` or `SCENARIOS` stay the size of the files.
#[test]
fn store_cards_are_apart_from_the_cards_with_files() {
    println!(
        "cards complete from the store: {}",
        CARDS.from_store().len()
    );
    assert!(!CARDS.from_store().is_empty());
    for def in CARDS.from_store() {
        assert!(
            !CARDS.cards().iter().any(|written| written.name == def.name),
            "{} has a file and a store definition",
            def.name
        );
        assert!(!SCENARIOS.iter().any(|(name, _)| *name == def.name));
    }
}

#[test]
fn a_decklist_of_store_cards_starts_a_game() {
    let deck = &[(20, crate::names::PLAINS), (20, VANILLA)];
    let table = GameTable::new(&CARDS, [deck, deck]).unwrap();
    State::new(&table, 1, beryl_engine::Profile::SMART).unwrap();
}
