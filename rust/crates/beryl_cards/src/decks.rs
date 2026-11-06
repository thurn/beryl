//! Decklists.

use beryl_engine::Decklist;
use beryl_engine::card::{CardDef, Supertypes};
use beryl_engine::rng::SplitMix64;

use crate::{CARDS, names};

/// The 40-card main deck of `docs/sample-decks/2025-11-18-17lands-cb8d5d5f-wu-control.txt`, the
/// Milestone 1 mirror: a mythic drafter's deck from the Arena Powered Cube run of 2025-10-30.
pub const WU_CONTROL: &Decklist = &[
    (1, names::JACE_VRYNS_PRODIGY),
    (1, names::SNAPCASTER_MAGE),
    (1, names::TRINKET_MAGE),
    (1, names::ABHORRENT_OCULUS),
    (1, names::BEZA_THE_BOUNDING_SPRING),
    (1, names::SUBTLETY),
    (1, names::COUNTERSPELL),
    (1, names::NO_MORE_LIES),
    (1, names::REPRIEVE),
    (1, names::OUST),
    (1, names::THIRST_FOR_DISCOVERY),
    (1, names::STOCK_UP),
    (1, names::DAMN),
    (1, names::WINDS_OF_ABANDON),
    (1, names::FRACTURED_IDENTITY),
    (1, names::TEFERI_TIME_RAVELER),
    (1, names::THE_WANDERING_EMPEROR),
    (1, names::JACE_WIELDER_OF_MYSTERIES),
    (1, names::JACE_THE_MIND_SCULPTOR),
    (1, names::TEFERI_HERO_OF_DOMINARIA),
    (1, names::MANA_CRYPT),
    (1, names::PORTABLE_HOLE),
    (1, names::CURRENCY_CONVERTER),
    (1, names::STAFF_OF_THE_STORYTELLER),
    (1, names::TUNDRA),
    (1, names::SEACHROME_COAST),
    (1, names::FLOODFARM_VERGE),
    (1, names::SPARAS_HEADQUARTERS),
    (6, names::PLAINS),
    (6, names::ISLAND),
];

/// The 40-card main deck of `docs/sample-decks/2026-10-17-PrinceJools-wu-control.txt`, the
/// MTGO Vintage Cube deck that was the mirror before. It plays implemented cards the mirror
/// lacks, which UI fixtures reach in play.
pub const VINTAGE_WU_CONTROL: &Decklist = &[
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
