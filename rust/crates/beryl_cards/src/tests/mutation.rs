//! The mutation check: every change to a card's effect data changes what one of the card's
//! scenarios shows.
//!
//! A definition can say a clause while no scenario shows that the clause does anything: Damn
//! once said "can't be regenerated" with no regeneration shield anywhere in its scenarios, so a
//! plain destroy would have passed every check. This test builds each mutant of each card file
//! ([`neighbours`]), plays the card's scenarios with the mutant in place of the card (those in
//! its file, the rules library's for it and its generated ones) in Beryl alone, and calls the
//! mutant killed when some scenario fails or ends in a different final snapshot. A mutant that
//! survives is a clause no scenario tells from its neighbour.
//!
//! A survivor has two remedies. Either a witness is missing, a position under which the two
//! values give different outcomes: add it to the generator (`derive::witness`), with a real
//! card implemented for it if the pool has none. Or the two definitions are truly
//! equivalent: list the mutant in [`EQUIVALENT`] with the reason.

use std::collections::BTreeSet;

use beryl_engine::CardTable;
use beryl_engine::card::CardDef;
use beryl_scenario::{Scenario, outcome};

use crate::CARDS;

mod neighbours;

use neighbours::mutants;

/// The mutants no scenario can tell from their card, because the two definitions do the same
/// thing in every game: the card, the node and the neighbour as the failure names them, and
/// why. An entry a scenario kills fails too, so the list stays current.
const EQUIVALENT: &[(&str, &str, &str, &str)] = &[
    (
        "Teferi, Hero of Dominaria",
        "ability 0, step 1",
        "Amount::Fixed(1)",
        "the delayed trigger it makes (ability 3) untaps two lands and reads no \
         `Amount::Carried`, so the amount the delay carries is never read",
    ),
    (
        "Winds of Abandon",
        "ability 0, step 1",
        "Players::EachOpponent",
        "only creatures you don't control are exiled, so you control none of them and search \
         for zero cards, and a search for zero cards does nothing",
    ),
    (
        "Winds of Abandon",
        "ability 1, step 1",
        "Players::EachOpponent",
        "only creatures you don't control are exiled, so you control none of them and search \
         for zero cards, and a search for zero cards does nothing",
    ),
];

/// A scenario's final snapshot, or why it failed, as text to compare.
fn result(cards: &'static CardTable, scenario: &Scenario) -> String {
    match outcome(cards, scenario) {
        Ok(snapshot) => format!("{snapshot:?}"),
        Err(failure) => format!("failed: {failure}"),
    }
}

/// Every scenario of the card: its file's, the rules library's for it and its generated ones.
fn scenarios_of(card: &str) -> Vec<&'static Scenario> {
    let own = crate::scenarios().iter().copied();
    let generated = crate::derive::all().iter().copied();
    own.chain(crate::library::by_card())
        .chain(generated)
        .filter(|(c, _)| *c == card)
        .flat_map(|(_, s)| s.iter())
        .collect()
}

/// What the check found for one card.
struct Report {
    card: &'static str,
    mutants: usize,
    /// Each survivor as "node: neighbour".
    survivors: Vec<String>,
}

/// Each scenario that passes with the real card, with its result.
fn expected(scenarios: Vec<&'static Scenario>) -> Vec<(&'static Scenario, String)> {
    // A scenario that fails with the real card is `every_card_has_scenarios_and_they_pass`'s to
    // report; it tells no mutant apart.
    scenarios
        .into_iter()
        .map(|s| (s, result(&CARDS, s)))
        .filter(|(_, r)| !r.starts_with("failed: "))
        .collect()
}

/// Whether some scenario fails, panics or ends otherwise with `mutant` in place of its card.
fn killed(mutant: &'static CardDef, expected: &[(&'static Scenario, String)]) -> bool {
    let table: &'static CardTable = Box::leak(Box::new(CardTable::overriding(
        &CARDS,
        Vec::leak(vec![mutant]),
    )));
    expected.iter().any(|(s, r)| {
        std::panic::catch_unwind(|| result(table, s)).map_or(true, |mutated| mutated != *r)
    })
}

/// Plays every mutant of `def` against its scenarios.
fn check(def: &'static CardDef) -> Report {
    let expected = expected(scenarios_of(def.name));
    let all = mutants(def);
    let survivors = all
        .iter()
        .filter(|m| !killed(m.def, &expected))
        .map(|m| format!("{}: {}", m.node, m.neighbour))
        .collect();
    Report {
        card: def.name,
        mutants: all.len(),
        survivors,
    }
}

/// Every card file's report, the cards shared among threads.
fn reports() -> Vec<Report> {
    let defs: &'static [&'static CardDef] = CARDS.cards();
    let threads = std::thread::available_parallelism().map_or(4, usize::from);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut reports: Vec<Report> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut out = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(&def) = defs.get(i) else { break };
                        out.push(check(def));
                    }
                    out
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("the mutation check panicked"))
            .collect()
    });
    reports.sort_by_key(|r| r.card);
    reports
}

#[test]
fn every_mutant_of_a_card_is_killed_by_its_scenarios() {
    let mut missed = Vec::new();
    let mut killed_but_listed: BTreeSet<String> = EQUIVALENT
        .iter()
        .map(|&(card, node, neighbour, _)| format!("{card}: {node}: {neighbour}"))
        .collect();
    let mut total = 0;
    for report in reports() {
        total += report.mutants;
        for survivor in report.survivors {
            let named = format!("{}: {survivor}", report.card);
            if !killed_but_listed.remove(&named) {
                missed.push(named);
            }
        }
    }
    assert!(
        missed.is_empty(),
        "{} of {total} mutants survive every scenario of their card:\n{}\nAdd a witness that \
         tells each from its card (rust/crates/beryl_cards/src/derive/witness.rs), or list it \
         in `EQUIVALENT` with why the two definitions always do the same.",
        missed.len(),
        missed.join("\n")
    );
    assert!(
        killed_but_listed.is_empty(),
        "a scenario now kills, so drop it from `EQUIVALENT`:\n{}",
        killed_but_listed.into_iter().collect::<Vec<_>>().join("\n")
    );
}

/// A Damn whose destruction lets creatures regenerate, the clause it once shipped without,
/// is told from Damn by its generated scenarios alone: the regeneration witness gives them a
/// creature with a shield.
#[test]
fn damn_that_allows_regeneration_is_killed_by_its_generated_scenarios() {
    let damn = CARDS.get(crate::names::DAMN).expect("Damn has a file");
    let generated = crate::derive::all().iter().filter(|(c, _)| *c == damn.name);
    let expected = expected(generated.flat_map(|(_, s)| s.iter()).collect());
    let allowing: Vec<_> = mutants(damn)
        .into_iter()
        .filter(|m| m.neighbour == "Regeneration::Allowed")
        .collect();
    // Its spell and its overloaded spell each destroy.
    assert_eq!(allowing.len(), 2);
    for mutant in allowing {
        assert!(killed(mutant.def, &expected), "{} survives", mutant.node);
    }
}

/// Teferi, Hero of Dominaria's emblem is a card of its own, listed among what Teferi creates:
/// a Teferi whose emblem's trigger may target nothing is a mutant of Teferi, and its scenarios
/// tell it from Teferi.
#[test]
fn a_mutant_of_an_emblem_is_a_mutant_of_its_card() {
    let teferi = CARDS
        .get(crate::names::TEFERI_HERO_OF_DOMINARIA)
        .expect("Teferi has a file");
    let emblem = teferi.tokens[0];
    let expected = expected(scenarios_of(teferi.name));
    let optional: Vec<_> = mutants(teferi)
        .into_iter()
        .filter(|m| m.node == format!("{}: ability 0, target 0", emblem.name))
        .collect();
    assert_eq!(optional.len(), 1);
    let mutant = &optional[0];
    let mut made = Vec::new();
    mutant.def.creates(&mut |token| made.push(token));
    assert!(made.iter().all(|&t| std::ptr::eq(t, mutant.def.tokens[0])));
    assert!(!std::ptr::eq(mutant.def.tokens[0], emblem));
    assert!(killed(mutant.def, &expected));
}
