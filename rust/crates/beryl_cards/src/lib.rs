//! Beryl's cards: one file per card under color directories (by the cube list's color
//! groups), the generated [`names`], the static [`CARDS`] table and the [`decks`].

// Bodies share the `Flow` signature of `Ability::body`, whether or not they can pause.
#![allow(clippy::unnecessary_wraps)]

pub mod azorius;
pub mod black;
pub mod blue;
pub mod colorless;
pub mod decks;
#[cfg(feature = "scenarios")]
pub mod generate;
pub mod multicolor;
pub mod names;
#[cfg(feature = "scenarios")]
pub mod props;
pub mod tokens;
pub mod white;

use beryl_engine::CardTable;

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
    &black::swamp::CARD,
    &white::damn::CARD,
    &white::winds_of_abandon::CARD,
    &blue::subtlety::CARD,
    &blue::snapcaster_mage::CARD,
    &azorius::teferi_hero_of_dominaria::CARD,
    &blue::jace_wielder_of_mysteries::CARD,
    &azorius::fractured_identity::CARD,
    &blue::abhorrent_oculus::CARD,
]);

/// Every card's scenarios (architecture §17.1), by card name.
#[cfg(feature = "scenarios")]
pub static SCENARIOS: &[(&str, &[beryl_scenario::Scenario])] = &[
    (names::PLAINS, white::plains::scenarios::SCENARIOS),
    (names::MANA_TITHE, white::mana_tithe::scenarios::SCENARIOS),
    (names::OUST, white::oust::scenarios::SCENARIOS),
    (names::GET_LOST, white::get_lost::scenarios::SCENARIOS),
    (
        names::ADELINE_RESPLENDENT_CATHAR,
        white::adeline_resplendent_cathar::scenarios::SCENARIOS,
    ),
    (
        names::ELITE_SPELLBINDER,
        white::elite_spellbinder::scenarios::SCENARIOS,
    ),
    (
        names::OVERLORD_OF_THE_MISTMOORS,
        white::overlord_of_the_mistmoors::scenarios::SCENARIOS,
    ),
    (names::ISLAND, blue::island::scenarios::SCENARIOS),
    (names::MANA_DRAIN, blue::mana_drain::scenarios::SCENARIOS),
    (
        names::SINK_INTO_STUPOR,
        blue::sink_into_stupor::scenarios::SCENARIOS,
    ),
    (names::TIME_SPIRAL, blue::time_spiral::scenarios::SCENARIOS),
    (names::TIMETWISTER, blue::timetwister::scenarios::SCENARIOS),
    (
        names::MALCOLM_ALLURING_SCOUNDREL,
        blue::malcolm_alluring_scoundrel::scenarios::SCENARIOS,
    ),
    (
        names::SPHINX_OF_FALSE_CONCLUSIONS,
        blue::sphinx_of_false_conclusions::scenarios::SCENARIOS,
    ),
    (
        names::THIEVING_SKYDIVER,
        blue::thieving_skydiver::scenarios::SCENARIOS,
    ),
    (
        names::THUNDERTRAP_TRAINER,
        blue::thundertrap_trainer::scenarios::SCENARIOS,
    ),
    (names::TUNDRA, azorius::tundra::scenarios::SCENARIOS),
    (
        names::NO_MORE_LIES,
        azorius::no_more_lies::scenarios::SCENARIOS,
    ),
    (
        names::SEACHROME_COAST,
        azorius::seachrome_coast::scenarios::SCENARIOS,
    ),
    (
        names::SPARAS_HEADQUARTERS,
        multicolor::sparas_headquarters::scenarios::SCENARIOS,
    ),
    (
        names::MANA_CRYPT,
        colorless::mana_crypt::scenarios::SCENARIOS,
    ),
    (
        names::STRIP_MINE,
        colorless::strip_mine::scenarios::SCENARIOS,
    ),
    (
        names::TEFERI_TIME_RAVELER,
        azorius::teferi_time_raveler::scenarios::SCENARIOS,
    ),
    (
        names::THE_WANDERING_EMPEROR,
        white::the_wandering_emperor::scenarios::SCENARIOS,
    ),
    (
        names::JACE_THE_MIND_SCULPTOR,
        blue::jace_the_mind_sculptor::scenarios::SCENARIOS,
    ),
    (
        names::JACE_VRYNS_PRODIGY,
        blue::jace_vryns_prodigy::scenarios::SCENARIOS,
    ),
    (
        names::STAFF_OF_THE_STORYTELLER,
        white::staff_of_the_storyteller::scenarios::SCENARIOS,
    ),
    (
        names::SWIFT_RECONFIGURATION,
        white::swift_reconfiguration::scenarios::SCENARIOS,
    ),
    (
        names::AANGS_ICEBERG,
        white::aangs_iceberg::scenarios::SCENARIOS,
    ),
    (
        names::COUNTERSPELL,
        blue::counterspell::scenarios::SCENARIOS,
    ),
    (names::REPRIEVE, white::reprieve::scenarios::SCENARIOS),
    (names::STOCK_UP, blue::stock_up::scenarios::SCENARIOS),
    (
        names::THIRST_FOR_DISCOVERY,
        blue::thirst_for_discovery::scenarios::SCENARIOS,
    ),
    (
        names::FLOODFARM_VERGE,
        azorius::floodfarm_verge::scenarios::SCENARIOS,
    ),
    (
        names::PORTABLE_HOLE,
        white::portable_hole::scenarios::SCENARIOS,
    ),
    (
        names::TRINKET_MAGE,
        blue::trinket_mage::scenarios::SCENARIOS,
    ),
    (
        names::BEZA_THE_BOUNDING_SPRING,
        white::beza_the_bounding_spring::scenarios::SCENARIOS,
    ),
    (
        names::CURRENCY_CONVERTER,
        colorless::currency_converter::scenarios::SCENARIOS,
    ),
    (names::SWAMP, black::swamp::scenarios::SCENARIOS),
    (names::DAMN, white::damn::scenarios::SCENARIOS),
    (
        names::WINDS_OF_ABANDON,
        white::winds_of_abandon::scenarios::SCENARIOS,
    ),
    (names::SUBTLETY, blue::subtlety::scenarios::SCENARIOS),
    (
        names::SNAPCASTER_MAGE,
        blue::snapcaster_mage::scenarios::SCENARIOS,
    ),
    (
        names::TEFERI_HERO_OF_DOMINARIA,
        azorius::teferi_hero_of_dominaria::scenarios::SCENARIOS,
    ),
    (
        names::JACE_WIELDER_OF_MYSTERIES,
        blue::jace_wielder_of_mysteries::scenarios::SCENARIOS,
    ),
    (
        names::FRACTURED_IDENTITY,
        azorius::fractured_identity::scenarios::SCENARIOS,
    ),
    (
        names::ABHORRENT_OCULUS,
        blue::abhorrent_oculus::scenarios::SCENARIOS,
    ),
];

#[cfg(test)]
mod tests;

/// Every ability, granted ability and casting option of the cards and their tokens, each
/// named for a report and with the address `beryl_engine::coverage` records it by: what a
/// game can exercise (architecture §17.3).
pub fn parts() -> Vec<(String, usize)> {
    use beryl_engine::card::CardDef;
    use beryl_engine::coverage::address;

    let mut defs: Vec<&'static CardDef> = Vec::new();
    for &def in CARDS.cards() {
        for &d in std::iter::once(&def).chain(def.tokens) {
            if !defs.iter().any(|&known| std::ptr::eq(known, d)) {
                defs.push(d);
            }
        }
    }
    let mut parts = Vec::new();
    for def in defs {
        for (f, face) in def.faces.iter().enumerate() {
            let name = if f == 0 { def.name } else { face.name };
            for (i, a) in face.abilities.iter().enumerate() {
                parts.push((format!("{name}: ability {i} ({:?})", a.kind), address(a)));
            }
            for (i, a) in face.grants.iter().enumerate() {
                let what = format!("{name}: granted ability {i} ({:?})", a.kind);
                parts.push((what, address(a)));
            }
            for (i, option) in face.casting.iter().enumerate() {
                let what = format!("{name}: casting option {i} ({option:?})");
                parts.push((what, address(option)));
            }
        }
    }
    parts
}
