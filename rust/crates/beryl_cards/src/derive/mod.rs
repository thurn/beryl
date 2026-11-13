//! Generated scenarios for the cards built only from library effects: "Library
//! effects carry hand-written scenarios; a card built only from them gets generated
//! scenarios."
//!
//! A card's are read from its definition and played, never written, and no seed decides what
//! they cover. Each starts from one of a few positions built from basic lands, the designated
//! props ([`crate::props`]) and the card itself ([`position`]): the card in its owner's hand,
//! on the battlefield (either face up), in the graveyard, or as the mana that pays for a prop.
//! From each, every way the card can be played at its owner's first priority in the main phase
//! opens a scenario (cast with each set of its casting options, played as a land, each ability
//! activated), as does each of those in response to a spell where it had none, and each event
//! its triggered abilities wait for that the position can bring about: an attack, a step, a
//! spell cast ([`play`]). What follows is played to a quiet position, the stack resolved, and
//! on to the end step where the card has an ability that waits for a step.
//!
//! Every question asked on the way (a target, a mode, a "may", an "unless", a payment, an
//! attacker or a blocker) gets each answer the scenario vocabulary can give it: the first run
//! of an opening takes each question's first answer, and each answer a run passed over is
//! taken by a later run that follows it to that question, unless an earlier run gave the same
//! question that answer. So every answer of every question is taken once, in the context that
//! asked it, with no product of them all.
//!
//! The runs are played through [`Live`](beryl_scenario::Live), so each script replays as
//! written in the strict runner; the dump adds each one's final snapshot for the oracles to
//! compare (architecture §17.2). A card with a Rust body gets none: [`custom`] lists them.

use std::collections::{BTreeSet, VecDeque};
use std::sync::OnceLock;

use beryl_engine::DecisionKind;
use beryl_engine::card::{Behavior, CardDef};
use beryl_scenario::{Action, Scenario};

use crate::CARDS;

mod play;
mod position;
mod witness;

use play::{Answers, Walk, openings, walk};
use position::{Place, early, preamble, start};
use witness::{Witness, witnesses};

/// What every generated scenario's name starts with, which the oracles' report marks them by.
pub const PREFIX: &str = "generated";

/// The most runs one opening plays, a bound on what a card with many questions adds to the
/// tests and the oracle lane. The answers past it, in the order the runs come to them, are left
/// untaken and counted in [`Generated::cut`]: today twelve, of two cards, all of them choices
/// of cards where a run before took others.
const MOST_RUNS: usize = 16;

/// One card's generated scenarios.
pub struct Generated {
    pub scenarios: Vec<Scenario>,
    /// The runs that could not be played, each with why: a bug in the generator or the
    /// runner.
    pub failures: Vec<String>,
    /// How many answers [`MOST_RUNS`] left untaken.
    pub cut: usize,
}

/// Every card with a Rust body anywhere on its faces or its tokens: none of these gets
/// generated scenarios, as nothing can read what its body does.
pub fn custom() -> Vec<&'static str> {
    CARDS
        .cards()
        .iter()
        .filter(|def| has_custom_body(def))
        .map(|def| def.name)
        .collect()
}

fn has_custom_body(def: &CardDef) -> bool {
    std::iter::once(def)
        .chain(def.tokens.iter().copied())
        .flat_map(|d| d.faces)
        .flat_map(|face| face.abilities.iter().chain(face.grants))
        .any(|a| matches!(a.body(), Behavior::Custom(_)))
}

/// Every card's generated scenarios but those of [`custom`], by card name, in the order of
/// [`CARDS`], generated once, the cards shared among threads. It refuses a card a run of
/// which could not be played.
pub fn all() -> &'static [(&'static str, &'static [Scenario])] {
    static ALL: OnceLock<Vec<(&'static str, &'static [Scenario])>> = OnceLock::new();
    ALL.get_or_init(|| {
        let defs: Vec<&'static CardDef> = CARDS
            .cards()
            .iter()
            .copied()
            .filter(|def| !has_custom_body(def))
            .collect();
        let threads = std::thread::available_parallelism().map_or(4, usize::from);
        let chunk = defs.len().div_ceil(threads).max(1);
        let generated: Vec<(&'static CardDef, Generated)> = std::thread::scope(|scope| {
            let handles: Vec<_> = defs
                .chunks(chunk)
                .map(|chunk| {
                    scope.spawn(move || {
                        chunk
                            .iter()
                            .map(|&def| (def, for_card(def)))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|h| h.join().expect("generation panicked"))
                .collect()
        });
        generated
            .into_iter()
            .map(|(def, generated)| {
                assert!(
                    generated.failures.is_empty(),
                    "{}: generated runs that could not be played:\n{}",
                    def.name,
                    generated.failures.join("\n")
                );
                (def.name, &*Vec::leak(generated.scenarios))
            })
            .collect()
    })
}

/// Where the scenarios of `def` start: each place of [`Place::of`], and each place the card is
/// played from again with each of its witnesses. A witness's prelude is played in the main
/// phase, so a place whose scenarios start before it, or where the card pays for another
/// spell, gets none, and neither does a witness that names the card itself.
fn positions(
    def: &'static CardDef,
    witnesses: &[&'static Witness],
) -> Vec<(Place, Option<&'static Witness>)> {
    let mut out = Vec::new();
    for place in Place::of(def) {
        out.push((place, None));
        if matches!(place, Place::Mana(..)) || early(def, place) {
            continue;
        }
        for &witness in witnesses {
            let names_card = witness.hand.contains(&def.name)
                || witness
                    .mine
                    .iter()
                    .chain(witness.theirs)
                    .any(|p| p.card == def.name);
            if !names_card {
                out.push((place, Some(witness)));
            }
        }
    }
    out
}

/// `def`'s generated scenarios.
pub fn for_card(def: &'static CardDef) -> Generated {
    let mut generated = Generated {
        scenarios: Vec::new(),
        failures: Vec::new(),
        cut: 0,
    };
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let witnesses = witnesses(def);
    for (place, witness) in positions(def, &witnesses) {
        let start = start(def, place, witness);
        let preamble = preamble(def, place);
        let prelude = witness.map_or(&[][..], |w| w.prelude);
        let words = match witness {
            Some(w) => format!("{} {}", place.words(), w.words),
            None => place.words(),
        };
        'openings: for (label, stimuli) in openings(def, place, &start, prelude, &preamble) {
            // The runs still to play, each a walk to go on with and the answer it is to give
            // the question it stopped at, and the answers some run gave or will give.
            let mut pending: VecDeque<(Walk, Option<usize>)> = VecDeque::new();
            match Walk::new(&start, prelude) {
                Ok(first) => pending.push_back((first, None)),
                Err(e) => generated.failures.push(format!("{words}: {e}")),
            }
            let mut taken = Answers::new();
            let mut runs = 0;
            while let Some((from, answer)) = pending.pop_front() {
                if runs == MOST_RUNS {
                    generated.cut += pending.len() + 1;
                    break;
                }
                runs += 1;
                let mut branches = Vec::new();
                let run = match walk(def, &stimuli, from, answer, &mut taken, &mut branches) {
                    Ok(walked) => walked.finish(def, place),
                    Err(e) => {
                        generated.failures.push(format!("{label} {words}: {e}"));
                        break;
                    }
                };
                if !matches!(place, Place::Mana(..)) {
                    pending.extend(branches.into_iter().map(|(w, answer)| (w, Some(answer))));
                }
                // A block among several attackers, which neither oracle's exporter can say,
                // leaves the run out, as does a question after triggers ordered by name.
                let block = |a: &Action| matches!(a, Action::Answer { kind, .. } if *kind == DecisionKind::BlockTarget);
                if run.unordered || run.actions.iter().any(block) {
                    continue;
                }
                if !run.touched || !seen.insert(format!("{place:?} {:?}", run.actions)) {
                    continue;
                }
                let name = String::leak(format!(
                    "{PREFIX} {}: {label} {words}",
                    generated.scenarios.len() + 1,
                ));
                let scenario = Scenario {
                    name,
                    actions: Vec::leak(run.actions),
                    ..start
                };
                generated.scenarios.push(scenario);
                if let Place::Mana(..) = place {
                    // As mana, the card's part is the same whatever the spell asks: one run
                    // of the first spell it pays for is enough.
                    break 'openings;
                }
            }
        }
    }
    generated
}
