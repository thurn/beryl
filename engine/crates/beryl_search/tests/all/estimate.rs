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

#[test]
fn an_extra_turn_or_combat_is_worth_more_than_a_vanilla_creature_of_its_mana_value() {
    // Final Fortune ({R}{R}) gives an extra turn, Relentless Assault ({2}{R}{R}) an extra
    // combat; Grizzly Bears is a 2/2 for {1}{G}, Hill Giant a 3/3 for {3}{R}.
    let [turn, bears, combat, giant] = values([
        names::FINAL_FORTUNE,
        "Grizzly Bears",
        names::RELENTLESS_ASSAULT,
        "Hill Giant",
    ]);
    assert!(turn > bears, "{turn} {bears}");
    assert!(combat > giant, "{combat} {giant}");
}

/// A transforming card is worth more than its front alone. Delver of Secrets is a 1/1 for {U}
/// that may become a 3/2 flyer, Savannah Lions a 2/1 for {W}: valued by its front, Delver was
/// worth less than the Lions under both tiers of card knowledge.
#[test]
fn a_transforming_card_is_worth_more_than_its_front() {
    for knowledge in [Knowledge::Estimate, Knowledge::Tuned] {
        let cards = [names::DELVER_OF_SECRETS, "Savannah Lions"];
        let deck: Vec<_> = cards.iter().map(|&card| (1, card)).collect();
        let t = GameTable::new(Format::CONSTRUCTED, &CARDS, [&deck, &deck]).unwrap();
        let values = CardValues::with(&t, knowledge);
        let [delver, lions] =
            cards.map(|name| values.get(t.index_named::<CardIx>(name).unwrap()).value);
        assert!(delver > lions, "{knowledge:?}: {delver} {lions}");
    }
}
