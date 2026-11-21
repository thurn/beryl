//! The estimate orders cards as a player would. These compare pairs and never assert an
//! exact value, so a refit of the weights keeps them passing as long as the order is sane.
//! They read the values the derived-only tier gives a table's cards ([`Knowledge::Estimate`]).

use beryl_cards::{CARDS, names};
use beryl_engine::{CardIx, Format, GameTable};
use beryl_search::{CardValues, Knowledge};

/// The estimated value of each card in `cards`, in order.
fn values<const N: usize>(cards: [&'static str; N]) -> [i16; N] {
    let deck: Vec<_> = cards.iter().map(|&card| (1, card)).collect();
    let t = GameTable::new(Format::CONSTRUCTED, &CARDS, [&deck, &deck]).unwrap();
    let values = CardValues::with(&t, Knowledge::Estimate);
    cards.map(|name| {
        let card = t
            .index_named::<CardIx>(name)
            .unwrap_or_else(|| panic!("{name} is in the table"));
        values.get(card).value
    })
}

#[test]
fn a_flyer_is_worth_more_than_the_same_body_on_the_ground() {
    // Wind Drake is a 2/2 flyer for {2}{U}; Grizzly Bears a 2/2 for {1}{G}, one mana less.
    let [flyer, ground] = values(["Wind Drake", "Grizzly Bears"]);
    assert!(flyer > ground);
}

#[test]
fn a_counterspell_is_worth_more_than_a_vanilla_one_one() {
    let [counter, vanilla] = values([names::COUNTERSPELL, "Savannah Lions"]);
    assert!(counter > vanilla);
}

#[test]
fn a_basic_land_is_worth_about_ten() {
    for (land, value) in [names::ISLAND, names::PLAINS]
        .into_iter()
        .zip(values([names::ISLAND, names::PLAINS]))
    {
        assert!((5..=15).contains(&value), "{land}: {value}");
    }
}

#[test]
fn removal_is_worth_more_than_the_same_mana_doing_nothing_to_the_board() {
    // Get Lost destroys a creature, enchantment or planeswalker for {1}{W}{W}.
    let [removal, bears] = values([names::GET_LOST, "Grizzly Bears"]);
    assert!(removal > bears);
}

#[test]
fn a_planeswalker_is_worth_more_than_a_vanilla_creature_of_its_mana_value() {
    let [walker, giant] = values([names::JACE_THE_MIND_SCULPTOR, "Hill Giant"]);
    assert!(walker > giant);
}
