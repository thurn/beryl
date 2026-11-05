//! Decklists.

use beryl_engine::Decklist;
use beryl_engine::card::{CardDef, Supertypes};
use beryl_engine::rng::SplitMix64;

use crate::{CARDS, names};

/// The 40-card main deck of `docs/sample-decks/2026-10-17-PrinceJools-wu-control.txt`, the
/// Milestone 1 mirror.
pub const WU_CONTROL: &Decklist = &[
    (1, names::MANA_TITHE),
    (1, names::SWIFT_RECONFIGURATION),
    (1, names::OUST),
    (1, names::THUNDERTRAP_TRAINER),
    (1, names::JACE_VRYNS_PRODIGY),
    (1, names::THIEVING_SKYDIVER),
    (1, names::MALCOLM_ALLURING_SCOUNDREL),
    (1, names::OVERLORD_OF_THE_MISTMOORS),
    (1, names::ELITE_SPELLBINDER),
    (1, names::TEFERI_TIME_RAVELER),
    (1, names::AANGS_ICEBERG),
    (1, names::ADELINE_RESPLENDENT_CATHAR),
    (1, names::THE_WANDERING_EMPEROR),
    (1, names::JACE_THE_MIND_SCULPTOR),
    (1, names::SPHINX_OF_FALSE_CONCLUSIONS),
    (1, names::TIME_SPIRAL),
    (1, names::STRIP_MINE),
    (1, names::MANA_CRYPT),
    (1, names::SEACHROME_COAST),
    (1, names::SPARAS_HEADQUARTERS),
    (1, names::TUNDRA),
    (7, names::PLAINS),
    (6, names::ISLAND),
    (1, names::STAFF_OF_THE_STORYTELLER),
    (1, names::GET_LOST),
    (1, names::NO_MORE_LIES),
    (1, names::MANA_DRAIN),
    (1, names::SINK_INTO_STUPOR),
    (1, names::TIMETWISTER),
];

/// The size of a fuzz deck: the minimum for limited play
/// ([CR 100.2b](https://yawgatog.com/resources/magic-rules/#R1002b)): "each deck has a
/// minimum deck size of 40 cards."
const FUZZ_DECK: usize = 40;

/// Nonbasic cards in a fuzz deck at most, about a cube deck's spells and nonbasic lands.
const FUZZ_NONBASICS: usize = 23;

/// A legal 40-card deck of the cards implemented so far, for random play: up to 23 nonbasic
/// cards chosen by `seed`, each at most once as in a cube draft, and basic lands in turn for
/// the rest. A limited deck holds "as many duplicates of a card as are included with the
/// product" (CR 100.2b), and the cube holds one of each.
pub fn fuzz_deck(seed: u64) -> Vec<(u8, &'static str)> {
    let (basics, mut nonbasics): (Vec<&CardDef>, Vec<&CardDef>) = CARDS
        .cards()
        .iter()
        .partition(|c| c.front().supertypes.contains(Supertypes::BASIC));
    SplitMix64::new(seed).shuffle(&mut nonbasics);
    nonbasics.truncate(FUZZ_NONBASICS);
    let mut deck: Vec<(u8, &'static str)> = nonbasics.iter().map(|c| (1, c.name)).collect();
    let lands = FUZZ_DECK - nonbasics.len();
    for (i, c) in basics.iter().enumerate() {
        let count = lands / basics.len() + usize::from(i < lands % basics.len());
        deck.push((count as u8, c.name));
    }
    deck
}
