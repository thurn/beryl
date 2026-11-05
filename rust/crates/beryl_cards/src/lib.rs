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
];

#[cfg(test)]
mod stand_ins;
#[cfg(test)]
mod tests;
