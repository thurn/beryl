//! The estimate orders cards as a player would. These compare pairs and never assert an
//! exact value, so a refit of the weights keeps them passing as long as the order is sane.

use beryl_cards::{CARDS, names};
use beryl_engine::card::CardDef;

use super::estimate;

fn card(name: &str) -> &'static CardDef {
    CARDS
        .get(name)
        .unwrap_or_else(|| panic!("{name} is in the card table"))
}

fn value(name: &str) -> i16 {
    estimate(card(name))
}

#[test]
fn a_flyer_is_worth_more_than_the_same_body_on_the_ground() {
    // Wind Drake is a 2/2 flyer for {2}{U}; Grizzly Bears a 2/2 for {1}{G}, one mana less.
    assert!(value("Wind Drake") > value("Grizzly Bears"));
}

#[test]
fn a_counterspell_is_worth_more_than_a_vanilla_one_one() {
    assert!(value(names::COUNTERSPELL) > value("Savannah Lions"));
}

#[test]
fn a_basic_land_is_worth_about_ten() {
    for land in [names::ISLAND, names::PLAINS] {
        assert!((5..=15).contains(&value(land)), "{land}: {}", value(land));
    }
}

#[test]
fn removal_is_worth_more_than_the_same_mana_doing_nothing_to_the_board() {
    // Get Lost destroys a creature, enchantment or planeswalker for {1}{W}{W}.
    assert!(value(names::GET_LOST) > value("Grizzly Bears"));
}

#[test]
fn a_planeswalker_is_worth_more_than_a_vanilla_creature_of_its_mana_value() {
    assert!(value(names::JACE_THE_MIND_SCULPTOR) > value("Hill Giant"));
}
