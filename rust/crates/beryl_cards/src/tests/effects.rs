//! Every instruction a card's definition holds is carried out by one of that card's own
//! scenarios.
//!
//! A definition can say more than its scenarios ever reach: Thieving Skydiver's "if that
//! artifact is an Equipment, attach it to this creature" was once an `If` that no scenario
//! entered, so a wrong attach would have passed in Beryl, `XMage` and Forge alike. This test walks
//! each card's effect data, every branch of every `If` and the inside of every `May`, `Unless`
//! and `ForEachPlayer`, and fails on any part that none of the card's scenarios (its file's and
//! the rules library's for it) carried out. A branch that is never taken is a branch whose
//! rules were never checked against `XMage` and Forge.

use beryl_engine::card::{Ability, AbilityKind, Behavior, CardDef, Effect};
use beryl_engine::coverage::{self, address};

use crate::CARDS;

/// What no scenario of the card can reach yet, and why: an entry a scenario reaches fails too,
/// so the list stays current. Each names the card, then the part as [`parts`] names it.
const UNREACHABLE: &[(&str, &str)] = &[];

/// Every instruction of an ability's effect data, named by where it is: "ability 1, step 2,
/// then".
fn instructions(effect: &'static Effect, at: &str, out: &mut Vec<(String, usize)>) {
    out.push((at.to_owned(), address(effect)));
    match *effect {
        Effect::Sequence(effects) => {
            for (i, effect) in effects.iter().enumerate() {
                instructions(effect, &format!("{at}, step {i}"), out);
            }
        }
        Effect::If {
            then, otherwise, ..
        } => {
            instructions(then, &format!("{at}, then"), out);
            if let Some(otherwise) = otherwise {
                instructions(otherwise, &format!("{at}, otherwise"), out);
            }
        }
        Effect::May { effect, .. }
        | Effect::Unless { effect, .. }
        | Effect::ForEachPlayer { effect, .. } => {
            instructions(effect, &format!("{at}, inside"), out);
        }
        _ => {}
    }
}

/// Every instruction of the card's abilities, granted abilities and tokens' abilities.
fn parts(def: &'static CardDef) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    // A mana ability makes its mana and a static ability applies without the interpreter,
    // and `every_ability_and_casting_option_is_exercised_by_a_scenario` holds them to a
    // scenario. An ability that says nothing has nothing to carry out.
    let mut ability = |owner: &str, what: String, a: &'static Ability| {
        let carried = matches!(
            a.kind(),
            AbilityKind::Spell | AbilityKind::Activated | AbilityKind::Triggered
        );
        if let Behavior::Data(effect) = a.body()
            && carried
            && !matches!(effect, Effect::Sequence([]))
        {
            instructions(effect, &format!("{owner}{what}"), &mut out);
        }
    };
    for &d in std::iter::once(&def).chain(def.tokens) {
        let owner = if std::ptr::eq(d, def) {
            String::new()
        } else {
            format!("{} token: ", d.name)
        };
        for (f, face) in d.faces.iter().enumerate() {
            let face_name = if f == 0 {
                String::new()
            } else {
                format!("{}: ", face.name)
            };
            for (i, a) in face.abilities.iter().enumerate() {
                ability(&owner, format!("{face_name}ability {i}"), a);
            }
            for (i, a) in face.grants.iter().enumerate() {
                ability(&owner, format!("{face_name}granted ability {i}"), a);
            }
        }
    }
    out
}

#[test]
fn every_instruction_of_a_card_is_carried_out_by_its_own_scenarios() {
    let library: Vec<_> = crate::library::by_card().collect();
    let mut missed = Vec::new();
    let mut reached = Vec::new();
    for &(card, scenarios) in crate::scenarios() {
        let Some(def) = CARDS.cards().iter().copied().find(|d| d.name == card) else {
            continue;
        };
        coverage::start();
        let theirs = library.iter().filter(|(c, _)| *c == card);
        for scenario in scenarios.iter().chain(theirs.flat_map(|(_, s)| s.iter())) {
            // `every_card_has_scenarios_and_they_pass` reports failures.
            let _ = beryl_scenario::run(&CARDS, scenario);
        }
        let hits = coverage::take();
        for (part, at) in parts(def) {
            let known = UNREACHABLE.iter().any(|&(c, p)| c == card && p == part);
            match (hits.contains(&at), known) {
                (false, false) => missed.push(format!("{card}: {part}")),
                (true, true) => reached.push(format!("{card}: {part}")),
                _ => {}
            }
        }
    }
    assert!(
        missed.is_empty(),
        "no scenario of the card carries out:\n{}\nWrite a scenario that reaches each part (a \
         card in no deck may be implemented for it: see the card-writing skill).",
        missed.join("\n")
    );
    assert!(
        reached.is_empty(),
        "a scenario now carries out, so drop it from `UNREACHABLE`:\n{}",
        reached.join("\n")
    );
}
