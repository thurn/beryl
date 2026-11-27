//! The mutation check: a neighbouring definition changes what one of the card's
//! scenarios shows.
//!
//! A definition can say a clause while no scenario shows that the clause does anything: Damn
//! once said "can't be regenerated" with no regeneration shield anywhere in its scenarios, so a
//! plain destroy would have passed every check. This test builds each mutant of each card file
//! ([`neighbours`]), plays the card's scenarios with the mutant in place of the card (those in
//! its file, the rules library's for it and its generated ones) in Beryl alone, and calls the
//! mutant killed when a scenario fails or gameplay differs at a scripted position. A mutant that
//! survives is a clause no scenario tells from its neighbour.
//!
//! A survivor has two remedies. Either a witness is missing, a position under which the two
//! values give different outcomes: add it to the generator (`derive::witness`), with a real
//! card implemented for it if the pool has none, or write a scenario for a card-specific
//! interaction. If both definitions always play the same way, list the mutant in
//! `EQUIVALENT` with the reason.

#[cfg(test)]
use std::collections::BTreeSet;

use beryl_engine::card::CardDef;
use beryl_engine::state::{Derived, Flag, StackEntry};
use std::cell::RefCell;

use beryl_engine::layout::Table;
use beryl_engine::{
    Bitboard, CardTable, Large, LargeSlot, Layout, Relayout, Seat, SlotId, State, Step,
};
use beryl_scenario::{Failure, Scenario, outcome_with};

use crate::CARDS;

#[cfg(test)]
mod coverage;
mod neighbours;
#[cfg(test)]
mod witnesses;

use neighbours::mutants;

/// The mutants no scenario can tell from their card, because the two definitions do the same
/// thing in every game: the card, the node and the neighbour as the failure names them, and
/// why. An entry a scenario kills fails too, so the list stays current.
#[cfg(test)]
const EQUIVALENT: &[(&str, &str, &str, &str)] = &[
    (
        "Agatha's Soul Cauldron",
        "ability 0, rule",
        "rule governed filter 1 Players::EachPlayer",
        "the rule change applies only to the Cauldron's controller, and no creature in the pool \
         lets another player activate its abilities, so the activator is always the creature's \
         controller",
    ),
    (
        "Hardened Scales",
        "ability 0, replacement event",
        "Counters { objects: All([Types(Types(4)), Whose(You)]), kind: Some(PlusOne), only_effects: true }",
        "no cost or rule of a card in the pool puts +1/+1 counters on a creature: every +1/+1 \
         counter is put by an effect, so limiting the replacement to effects changes nothing",
    ),
    (
        "Aang's Iceberg",
        "ability 1, cost",
        "part 0 4",
        "waterbend can pay only the three generic mana in this activation cost",
    ),
    (
        "Swift Reconfiguration",
        "ability 2, modification",
        "0 SetSubtypeWord(SubtypeWord(Vehicle))",
        "after the Aura sets the permanent's type to artifact, Vehicle is the only artifact subtype an eligible permanent in this pool can already have",
    ),
    (
        "Swift Reconfiguration",
        "granted ability 0",
        "0 SetTypes(Types(5))",
        "crewing sets a Vehicle's types to artifact creature instead of adding them, and the \
         Aura has already made the permanent an artifact, so the two end with the same types",
    ),
    (
        "Double Major",
        "ability 0",
        "new_targets true",
        "no creature spell in the pool has a target, so a copy of one has nothing for its \
         controller to choose new targets for",
    ),
    (
        "Arcane Proxy",
        "casting option 0",
        "drop",
        "a prototyped cast reads the prototype option of the prototyped face, which keeps its \
         own list, so the printed face's copy of the option only makes the definition expect \
         a prototyped face and no game reads it",
    ),
    (
        "Mister Immortal",
        "ability 0",
        "under Players::You",
        "the card's owner activates the ability from the graveyard or exile, so the ability's \
         controller is also the returning card's owner",
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

/// Mutants of cards with no hand-written scenarios that their generated scenarios kill only at
/// scripted positions, which the oracles (`XMage` and Forge) never compare: the card, "node:
/// neighbour" as [`generated`] reports them, and why no oracle-visible fact tells them apart.
/// A definition with one of these neighbours in place of the real value passes every oracle
/// check too. The list may only shrink: a scenario or snapshot fact the oracles compare removes
/// an entry, and `every_card_has_scenarios_and_they_pass` fails for one no longer needed.
pub const ORACLE_BLIND: &[(&str, &str, &str)] = &[
    (
        "Jace, the Mind Sculptor",
        "ability 1, cost: drop part 0",
        "without its loyalty cost of 0 the ability is no longer a loyalty ability, which changes \
         only when and how often it may be activated, and the generated runs activate it once, \
         in a main phase",
    ),
    (
        "Strip Mine",
        "ability 1, cost: drop part 0",
        TAPPED_STRIP_MINE,
    ),
    (
        "Strip Mine",
        "ability 1, cost: part 0 sacrifice",
        TAPPED_STRIP_MINE,
    ),
    (
        "Time Spiral",
        "ability 0, step 3: Amount::Fixed(5)",
        "only a run that untaps six lands tells five from six, and the generated runs stop at \
         three picks, where the walk of alternative answers reaches its limit of runs",
    ),
    (
        "Time Spiral",
        "ability 0, step 3: Amount::Fixed(7)",
        "casting Time Spiral taps six lands, so at most six are tapped when it untaps, and \
         seven untaps those six too",
    ),
    (
        "Time Spiral",
        "ability 0, step 3: filter Filter::Any",
        "the generated runs untap only lands, which a permanent of any kind includes, and the \
         oracles replay those picks alone",
    ),
];

/// Why a change to the tap in Strip Mine's cost stays in [`ORACLE_BLIND`].
const TAPPED_STRIP_MINE: &str = "Strip Mine is sacrificed to pay the cost, so whether the cost \
    also taps it shows only when it is already tapped, and a run that taps it for mana before \
    activating it is one an oracle may pay differently";

/// Observes transient changes that the final snapshot cannot see, such as a trigger that
/// resolves earlier, or mana that a player spends before the scenario ends.
#[derive(PartialEq, Eq)]
struct Position {
    step: Step<Large>,
    stack: Vec<StackEntry<Large>>,
    mana: [[u8; 6]; 2],
    knowledge: [Bitboard<LargeSlot>; 2],
    permanents: Vec<(LargeSlot, Derived<Large>, bool, u8)>,
    subgame_depth: usize,
}

impl Position {
    fn of<L: Layout>(state: &State<L>, step: &Step<L>) -> Self {
        let permanents = state
            .battlefield()
            .iter()
            .copied()
            .map(|slot| {
                let object = state.object(slot);
                (
                    slot.to().expect("the slot fits in Large"),
                    state.derived(slot).relayout(),
                    object.flags().has(Flag::Tapped),
                    beryl_engine::UnsignedNumber::to_u8_saturating(object.damage()),
                )
            })
            .collect();
        let knowledge = state.knowledge();
        Self {
            step: step.relayout(),
            stack: state.stack().iter().map(Relayout::relayout).collect(),
            subgame_depth: state.subgame_depth(),
            knowledge: Seat::BOTH.map(|seat| {
                knowledge.as_slice()[seat.index()].fold(Bitboard::EMPTY, |mut known, slot| {
                    known.insert(slot.to().expect("the slot fits in Large"));
                    known
                })
            }),
            mana: Seat::BOTH.map(|seat| state.player(seat).mana().amounts_u8()),
            permanents,
        }
    }
}

#[derive(PartialEq, Eq)]
struct ScenarioResult {
    snapshot: String,
    positions: Vec<Position>,
}

fn result(cards: &'static CardTable, scenario: &Scenario) -> Result<ScenarioResult, Failure> {
    let positions = RefCell::new(Vec::new());
    let snapshot = outcome_with(
        cards,
        scenario,
        |state, step| positions.borrow_mut().push(Position::of(state, step)),
        |state, step| positions.borrow_mut().push(Position::of(state, step)),
    )?;
    Ok(ScenarioResult {
        snapshot: format!("{snapshot:?}"),
        positions: positions.into_inner(),
    })
}

/// What the check found for one card.
pub struct Report {
    pub card: &'static str,
    pub mutants: usize,
    /// Each survivor as "node: neighbour".
    pub survivors: Vec<String>,
    /// Each survivor that [`ORACLE_BLIND`] lists, as "node: neighbour".
    pub blind: Vec<String>,
}

/// Checks a card against its generated scenarios, counting only the kills the oracles
/// could see.
///
/// A generated scenario has no assertions of its own: what it shows about the card is the
/// final snapshot that the oracles compare. Its kill stands for "an oracle playing the real
/// card would disagree with this neighbour" only when the mutant fails the scenario or ends it
/// with another snapshot. A difference at a scripted position, such as which land paid an
/// equip cost, is one the oracles never compare. A Leonin Scimitar written with "Equip {0}"
/// passed every behavior check, because its generated scenarios told the `generic 0`
/// neighbour from the card only by a tapped land.
///
/// Survivors listed in [`ORACLE_BLIND`] move to [`Report::blind`].
pub fn generated(def: &'static CardDef, scenarios: &[Scenario]) -> Report {
    let mut report = report_with(def, &expected(scenarios.iter().collect()), Seen::ByOracles);
    let listed = |survivor: &String| {
        ORACLE_BLIND
            .iter()
            .any(|&(card, mutant, _)| card == def.name && mutant == survivor)
    };
    (report.blind, report.survivors) = report.survivors.into_iter().partition(listed);
    report
}

/// Each scenario that passes with the real card, with its result.
fn expected(scenarios: Vec<&Scenario>) -> Vec<(&Scenario, ScenarioResult)> {
    // A scenario that fails with the real card is `every_card_has_scenarios_and_they_pass`'s to
    // report; it tells no mutant apart.
    scenarios
        .into_iter()
        .filter_map(|s| result(&CARDS, s).ok().map(|result| (s, result)))
        .collect()
}

/// Which differences between a mutant's play and its card's count as a kill.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Seen {
    /// Any difference Beryl can observe, at a scripted position or in the final snapshot.
    #[cfg(test)]
    ByBeryl,
    /// Only a failed scenario or a different final snapshot, which the oracles compare.
    ByOracles,
}

/// Whether some scenario fails or plays differently with `mutant` in place of its card.
#[cfg(test)]
fn killed(mutant: &'static CardDef, expected: &[(&Scenario, ScenarioResult)]) -> bool {
    killed_as(mutant, expected, Seen::ByBeryl)
}

fn killed_as(
    mutant: &'static CardDef,
    expected: &[(&Scenario, ScenarioResult)],
    seen: Seen,
) -> bool {
    let table: &'static CardTable = Box::leak(Box::new(CardTable::overriding(
        &CARDS,
        Vec::leak(vec![mutant]),
    )));
    expected.iter().any(|(s, r)| {
        std::panic::catch_unwind(|| result(table, s)).map_or(true, |mutated| {
            mutated.map_or(true, |result| match seen {
                #[cfg(test)]
                Seen::ByBeryl => result != *r,
                Seen::ByOracles => result.snapshot != r.snapshot,
            })
        })
    })
}

/// Plays every mutant of `def` against its scenarios.
#[cfg(test)]
fn check(def: &'static CardDef) -> Report {
    let witnesses = witnesses::for_card(def.name);
    let written = crate::written_scenarios_for(def.name);
    let shared = crate::library::by_card();
    let generated = crate::derive::all().iter().copied();
    let scenarios = shared
        .chain(generated)
        .filter(|&(card, _)| card == def.name)
        .flat_map(|(_, cases)| cases);
    let mut expected = expected(written.iter().chain(scenarios).collect());
    expected.extend(witnesses.iter().map(|scenario| {
        let result = result(&CARDS, scenario)
            .unwrap_or_else(|error| panic!("{}: invalid witness: {error}", def.name));
        (scenario, result)
    }));
    report(def, &expected)
}

#[cfg(test)]
fn report(def: &'static CardDef, expected: &[(&Scenario, ScenarioResult)]) -> Report {
    report_with(def, expected, Seen::ByBeryl)
}

fn report_with(
    def: &'static CardDef,
    expected: &[(&Scenario, ScenarioResult)],
    seen: Seen,
) -> Report {
    let all = mutants(def);
    let survivors = all
        .iter()
        .filter(|m| !killed_as(m.def, expected, seen))
        .map(|m| format!("{}: {}", m.node, m.neighbour))
        .collect();
    Report {
        card: def.name,
        mutants: all.len(),
        survivors,
        blind: Vec::new(),
    }
}

/// Every card file's report, the cards shared among threads.
#[cfg(test)]
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
         tells each from its card (engine/crates/beryl_cards/src/derive/witness.rs), or list it \
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
    let expected = expected(crate::library::emblems::SCENARIOS.iter().collect());
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

/// A mutant only changes something a game can read. Show and Tell links what it puts onto the
/// battlefield, but nothing of a sorcery reads the link, so it has no `linked` mutant, while
/// Animate Dead, whose granted enchant ability needs the link, keeps its own. And Animate
/// Dead's granted enchant ability is never cast, so it has no mutants.
#[test]
fn mutants_change_only_what_a_game_reads() {
    let named = |card: &str| -> Vec<String> {
        let def = CARDS.get(card).expect("the card has a file");
        mutants(def)
            .into_iter()
            .map(|m| format!("{}: {}", m.node, m.neighbour))
            .collect()
    };
    let show_and_tell = named(crate::names::SHOW_AND_TELL);
    assert!(!show_and_tell.iter().any(|m| m.contains("linked")));
    let animate_dead = named(crate::names::ANIMATE_DEAD);
    assert!(animate_dead.iter().any(|m| m.ends_with("linked false")));
    assert!(
        !animate_dead
            .iter()
            .any(|m| m.starts_with("granted ability"))
    );
}
