//! The overlay names only cards that exist, and each tuned ability is still the one it was
//! tuned for.

use beryl_cards::{CARDS, names};

use super::{Expect, Shape, TUNINGS, Tuning};
use crate::{CardValues, Knowledge};

/// One line for each way `tunings` disagrees with the card definitions: a name no card has,
/// or a tuned ability that is missing or does not have the shape the tuning expects.
fn mismatches(tunings: &[Tuning]) -> Vec<String> {
    let mut found = Vec::new();
    for tuning in tunings {
        let Some(def) = CARDS.get(tuning.name) else {
            found.push(format!("{}: no such card", tuning.name));
            continue;
        };
        let abilities = def.front().abilities;
        for (tune, expect) in tuning.tuned() {
            match abilities.get(usize::from(expect.index)) {
                None => found.push(format!(
                    "{}: {tune:?} names ability {}, but the front face has {}",
                    tuning.name,
                    expect.index,
                    abilities.len()
                )),
                Some(a) if !expect.shape.matches(a) => found.push(format!(
                    "{}: {tune:?} expects ability {} to be {:?}, but it is {:?}",
                    tuning.name,
                    expect.index,
                    expect.shape,
                    a.kind()
                )),
                Some(_) => {}
            }
        }
    }
    found
}

#[test]
fn every_tuning_names_a_card_and_the_abilities_it_was_tuned_for() {
    assert_eq!(mismatches(TUNINGS), Vec::<String>::new());
}

/// A tuning written as though the card's two loyalty abilities were in the other order is
/// reported for both, by card and ability.
#[test]
fn a_tuning_on_reordered_abilities_is_reported() {
    let swapped = [Tuning::value(names::JACE_WIELDER_OF_MYSTERIES, 65)
        .tuned_as_not_drawing(Expect::ability(1, Shape::Loyalty(-8)))
        .tuned_as_not_drawing(Expect::ability(2, Shape::Loyalty(1)))];
    let found = mismatches(&swapped);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found[0].contains("ability 1"), "{found:?}");
    assert!(found[1].contains("ability 2"), "{found:?}");
    assert!(
        found
            .iter()
            .all(|line| line.starts_with(names::JACE_WIELDER_OF_MYSTERIES)),
        "{found:?}"
    );
}

/// A tuning past the front face's last ability, or for a card that does not exist, is
/// reported.
#[test]
fn a_missing_card_or_ability_is_reported() {
    let wrong = [
        Tuning::value(names::SPARAS_HEADQUARTERS, 12)
            .tuned_as_not_drawing(Expect::ability(5, Shape::Cycling)),
        Tuning::value("No Such Card", 1),
    ];
    assert_eq!(mismatches(&wrong).len(), 2);
}

#[test]
fn tuned_values_are_the_hand_tuned_ones() {
    let t = beryl_engine::GameTable::new(
        &CARDS,
        [
            beryl_cards::decks::WU_CONTROL,
            beryl_cards::decks::VINTAGE_WU_CONTROL,
        ],
    )
    .unwrap();
    let tuned = CardValues::with(&t, Knowledge::Tuned);
    let estimated = CardValues::with(&t, Knowledge::Estimate);
    let card = |name: &str| {
        t.indices::<beryl_engine::LargeCardIx>()
            .find(|&c| t.def(c).name == name)
            .unwrap()
    };
    assert_eq!(tuned.get(card(names::JACE_THE_MIND_SCULPTOR)).value, 90);
    assert_eq!(tuned.get(card(names::ISLAND)).value, 10);
    assert!(tuned.get(card(names::MANA_CRYPT)).never_cast);
    assert!(!estimated.get(card(names::MANA_CRYPT)).never_cast);
    assert_eq!(
        estimated.get(card(names::JACE_THE_MIND_SCULPTOR)).value,
        crate::estimate::estimate(t.def(card(names::JACE_THE_MIND_SCULPTOR)))
    );
}
