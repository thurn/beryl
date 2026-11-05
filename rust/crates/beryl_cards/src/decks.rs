//! Decklists.

use beryl_engine::Decklist;

use crate::names;

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
