//! The overlay names only cards that exist, and each tuned ability is still the one it was
//! tuned for.

use beryl_cards::{CARDS, decks, names};
use beryl_engine::{Format, GameTable, LargeCardIx};
use beryl_search::{CardValues, Knowledge, tuning_mismatches};

#[test]
fn every_tuning_names_a_card_and_the_abilities_it_was_tuned_for() {
    assert_eq!(
        tuning_mismatches(|name| CARDS.get(name)),
        Vec::<String>::new()
    );
}

#[test]
fn tuned_values_are_the_hand_tuned_ones() {
    let t = GameTable::new(
        Format::CONSTRUCTED,
        &CARDS,
        [decks::WU_CONTROL, decks::VINTAGE_WU_CONTROL],
    )
    .unwrap();
    let tuned = CardValues::with(&t, Knowledge::Tuned);
    let estimated = CardValues::with(&t, Knowledge::Estimate);
    let card = |name: &str| {
        t.indices::<LargeCardIx>()
            .find(|&c| t.def(c).name == name)
            .unwrap()
    };
    assert_eq!(tuned.get(card(names::JACE_THE_MIND_SCULPTOR)).value, 90);
    assert_eq!(tuned.get(card(names::ISLAND)).value, 10);
    assert!(tuned.get(card(names::MANA_CRYPT)).never_cast);
    assert!(!estimated.get(card(names::MANA_CRYPT)).never_cast);
    // The derived tier values Jace by its definition alone, where the tuned tier has its own
    // number for him.
    assert_ne!(
        estimated.get(card(names::JACE_THE_MIND_SCULPTOR)).value,
        tuned.get(card(names::JACE_THE_MIND_SCULPTOR)).value
    );
}
