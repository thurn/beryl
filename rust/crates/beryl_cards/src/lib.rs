//! Beryl's cards: one file per card under color directories (by the cube list's color
//! groups), the generated [`names`], the static [`CARDS`] table and the [`decks`].

// Bodies share the `Flow` signature of `Ability::body`, whether or not they can pause.
#![allow(clippy::unnecessary_wraps)]

pub mod azorius;
pub mod blue;
pub mod colorless;
pub mod decks;
pub mod multicolor;
pub mod names;
pub mod tokens;
pub mod white;

use beryl_engine::CardTable;
use beryl_scenario::Scenario;

/// Every card Beryl implements.
pub static CARDS: CardTable = CardTable::new(&[
    &white::plains::CARD,
    &white::mana_tithe::CARD,
    &white::oust::CARD,
    &white::get_lost::CARD,
    &white::adeline_resplendent_cathar::CARD,
    &white::elite_spellbinder::CARD,
    &white::overlord_of_the_mistmoors::CARD,
    &blue::island::CARD,
    &blue::mana_drain::CARD,
    &blue::sink_into_stupor::CARD,
    &blue::time_spiral::CARD,
    &blue::timetwister::CARD,
    &blue::malcolm_alluring_scoundrel::CARD,
    &blue::sphinx_of_false_conclusions::CARD,
    &blue::thieving_skydiver::CARD,
    &blue::thundertrap_trainer::CARD,
    &azorius::tundra::CARD,
    &azorius::no_more_lies::CARD,
    &azorius::seachrome_coast::CARD,
    &multicolor::sparas_headquarters::CARD,
    &colorless::mana_crypt::CARD,
    &colorless::strip_mine::CARD,
    &azorius::teferi_time_raveler::CARD,
    &white::the_wandering_emperor::CARD,
    &blue::jace_the_mind_sculptor::CARD,
    &blue::jace_vryns_prodigy::CARD,
    &white::staff_of_the_storyteller::CARD,
    &white::swift_reconfiguration::CARD,
    &white::aangs_iceberg::CARD,
    &blue::counterspell::CARD,
    &white::reprieve::CARD,
    &blue::stock_up::CARD,
    &blue::thirst_for_discovery::CARD,
    &azorius::floodfarm_verge::CARD,
    &white::portable_hole::CARD,
    &blue::trinket_mage::CARD,
    &white::beza_the_bounding_spring::CARD,
    &colorless::currency_converter::CARD,
]);

/// Every card's scenarios (architecture §17.1), by card name.
pub static SCENARIOS: &[(&str, &[Scenario])] = &[
    (names::PLAINS, white::plains::SCENARIOS),
    (names::MANA_TITHE, white::mana_tithe::SCENARIOS),
    (names::OUST, white::oust::SCENARIOS),
    (names::GET_LOST, white::get_lost::SCENARIOS),
    (
        names::ADELINE_RESPLENDENT_CATHAR,
        white::adeline_resplendent_cathar::SCENARIOS,
    ),
    (
        names::ELITE_SPELLBINDER,
        white::elite_spellbinder::SCENARIOS,
    ),
    (
        names::OVERLORD_OF_THE_MISTMOORS,
        white::overlord_of_the_mistmoors::SCENARIOS,
    ),
    (names::ISLAND, blue::island::SCENARIOS),
    (names::MANA_DRAIN, blue::mana_drain::SCENARIOS),
    (names::SINK_INTO_STUPOR, blue::sink_into_stupor::SCENARIOS),
    (names::TIME_SPIRAL, blue::time_spiral::SCENARIOS),
    (names::TIMETWISTER, blue::timetwister::SCENARIOS),
    (
        names::MALCOLM_ALLURING_SCOUNDREL,
        blue::malcolm_alluring_scoundrel::SCENARIOS,
    ),
    (
        names::SPHINX_OF_FALSE_CONCLUSIONS,
        blue::sphinx_of_false_conclusions::SCENARIOS,
    ),
    (names::THIEVING_SKYDIVER, blue::thieving_skydiver::SCENARIOS),
    (
        names::THUNDERTRAP_TRAINER,
        blue::thundertrap_trainer::SCENARIOS,
    ),
    (names::TUNDRA, azorius::tundra::SCENARIOS),
    (names::NO_MORE_LIES, azorius::no_more_lies::SCENARIOS),
    (names::SEACHROME_COAST, azorius::seachrome_coast::SCENARIOS),
    (
        names::SPARAS_HEADQUARTERS,
        multicolor::sparas_headquarters::SCENARIOS,
    ),
    (names::MANA_CRYPT, colorless::mana_crypt::SCENARIOS),
    (names::STRIP_MINE, colorless::strip_mine::SCENARIOS),
    (
        names::TEFERI_TIME_RAVELER,
        azorius::teferi_time_raveler::SCENARIOS,
    ),
    (
        names::THE_WANDERING_EMPEROR,
        white::the_wandering_emperor::SCENARIOS,
    ),
    (
        names::JACE_THE_MIND_SCULPTOR,
        blue::jace_the_mind_sculptor::SCENARIOS,
    ),
    (
        names::JACE_VRYNS_PRODIGY,
        blue::jace_vryns_prodigy::SCENARIOS,
    ),
    (
        names::STAFF_OF_THE_STORYTELLER,
        white::staff_of_the_storyteller::SCENARIOS,
    ),
    (
        names::SWIFT_RECONFIGURATION,
        white::swift_reconfiguration::SCENARIOS,
    ),
    (names::AANGS_ICEBERG, white::aangs_iceberg::SCENARIOS),
    (names::COUNTERSPELL, blue::counterspell::SCENARIOS),
    (names::REPRIEVE, white::reprieve::SCENARIOS),
    (names::STOCK_UP, blue::stock_up::SCENARIOS),
    (
        names::THIRST_FOR_DISCOVERY,
        blue::thirst_for_discovery::SCENARIOS,
    ),
    (names::FLOODFARM_VERGE, azorius::floodfarm_verge::SCENARIOS),
    (names::PORTABLE_HOLE, white::portable_hole::SCENARIOS),
    (names::TRINKET_MAGE, blue::trinket_mage::SCENARIOS),
    (
        names::BEZA_THE_BOUNDING_SPRING,
        white::beza_the_bounding_spring::SCENARIOS,
    ),
    (
        names::CURRENCY_CONVERTER,
        colorless::currency_converter::SCENARIOS,
    ),
];

#[cfg(test)]
mod tests;
