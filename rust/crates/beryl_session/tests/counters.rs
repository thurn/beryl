//! Which cards counter the spell they target, as the engine knows it
//! ([`Ability::takes_target`]), against their Oracle text: the fact is read from effect
//! data or declared beside a Rust body, and a body nothing declares for would aim at the
//! viewer's own spell unasked (UI Conventions, exception 7).

use beryl_card_store::STORE;
use beryl_cards::{CARDS, names};
use beryl_engine::card::{Ability, Behavior, SpecIx, Takes};

/// Whether `ability` counters what is chosen for one of its targets.
fn counters(ability: &Ability) -> bool {
    (0..ability.targets().len()).any(|target| {
        let target = u8::try_from(target).expect("a few targets");
        ability.takes_target(SpecIx::new(target), Takes::Counter)
    })
}

#[test]
fn a_card_counters_its_target_exactly_where_its_text_says_so() {
    let (mut read, mut declared) = (Vec::new(), Vec::new());
    for card in CARDS.cards() {
        let record = STORE.card(card.name).expect("a record for every card");
        assert_eq!(card.faces.len(), record.faces.len(), "{}", card.name);
        for (face, printed) in card.faces.iter().zip(&record.faces) {
            let text = printed.oracle_text.as_deref().unwrap_or_default();
            let says = text.to_lowercase().contains("counter target");
            let countering: Vec<&Ability> = face.abilities.iter().filter(|a| counters(a)).collect();
            assert_eq!(!countering.is_empty(), says, "{}: {text:?}", printed.name);
            for ability in countering {
                match ability.body() {
                    Behavior::Data(_) => read.push(card.name),
                    Behavior::Custom(_) => declared.push(card.name),
                }
            }
        }
    }
    // Both ways of knowing are in the pool.
    assert!(read.contains(&names::COUNTERSPELL), "{read:?}");
    assert!(declared.contains(&names::MANA_DRAIN), "{declared:?}");
}
