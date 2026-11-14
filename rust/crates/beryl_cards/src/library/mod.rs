//! Hand-written scenarios of the rules library's entries, in a tree that mirrors the library's modules. Rules code names no card and
//! the oracles play real ones, so an entry's scenario lives here: it uses basic lands, the
//! designated props ([`crate::props`]) and one card that has the effect.

use beryl_scenario::Scenario;

use crate::names;

pub mod actions;
pub mod casting;
pub mod choices;
pub mod coins;
pub mod continuous;
pub mod costs;
pub mod damage;
pub mod delayed_triggers;
pub mod emblems;
pub mod keywords;
#[expect(
    clippy::module_inception,
    reason = "the rules library's module for a library, CR 401"
)]
pub mod library;
pub mod linked;
pub mod mana;
pub mod tokens;
pub mod zones;

/// Every entry's scenarios: the library module, the card that has the effect, and the
/// scenarios, which the oracles play as that card's.
pub static SCENARIOS: &[(&str, &str, &[Scenario])] = &[
    (
        "actions::counter",
        names::NO_MORE_LIES,
        actions::counter::SCENARIOS,
    ),
    (
        "actions::explore",
        names::GET_LOST,
        actions::explore::SCENARIOS,
    ),
    (
        "actions::fateseal",
        names::JACE_THE_MIND_SCULPTOR,
        actions::fateseal::SCENARIOS,
    ),
    (
        "actions::search",
        names::WINDS_OF_ABANDON,
        actions::search::SCENARIOS,
    ),
    (
        "actions::shuffle",
        names::TIMETWISTER,
        actions::shuffle::SCENARIOS,
    ),
    (
        "actions::tap",
        names::TEFERI_HERO_OF_DOMINARIA,
        actions::tap::SCENARIOS,
    ),
    (
        "actions::manifest_dread",
        names::ABHORRENT_OCULUS,
        actions::manifest_dread::SCENARIOS,
    ),
    (
        "actions::transform",
        names::JACE_VRYNS_PRODIGY,
        actions::transform::SCENARIOS,
    ),
    (
        "casting",
        names::MALCOLM_ALLURING_SCOUNDREL,
        casting::SCENARIOS,
    ),
    ("casting", names::JACE_VRYNS_PRODIGY, casting::PERMITTED),
    ("choices", names::ELITE_SPELLBINDER, choices::SCENARIOS),
    ("coins", names::MANA_CRYPT, coins::SCENARIOS),
    (
        "continuous",
        names::TEFERI_TIME_RAVELER,
        continuous::SCENARIOS,
    ),
    ("costs", names::THIRST_FOR_DISCOVERY, costs::SCENARIOS),
    ("damage", names::MANA_CRYPT, damage::SCENARIOS),
    (
        "delayed_triggers",
        names::TEFERI_HERO_OF_DOMINARIA,
        delayed_triggers::SCENARIOS,
    ),
    (
        "emblems",
        names::TEFERI_HERO_OF_DOMINARIA,
        emblems::SCENARIOS,
    ),
    ("library", names::STOCK_UP, library::DIG),
    ("library", names::JACE_THE_MIND_SCULPTOR, library::PUT_BACK),
    ("library", names::SUBTLETY, library::TOP_OR_BOTTOM),
    (
        "keywords::evasion",
        keywords::evasion::FLYER,
        keywords::evasion::SCENARIOS,
    ),
    (
        "keywords::flashback",
        names::SNAPCASTER_MAGE,
        keywords::flashback::SCENARIOS,
    ),
    (
        "keywords::indestructible",
        keywords::indestructible::INDESTRUCTIBLE,
        keywords::indestructible::SCENARIOS,
    ),
    (
        "keywords::landwalk",
        keywords::landwalk::LANDWALKER,
        keywords::landwalk::SCENARIOS,
    ),
    (
        "keywords::lifelink",
        keywords::lifelink::LIFELINKER,
        keywords::lifelink::SCENARIOS,
    ),
    (
        "keywords::trample",
        keywords::trample::TRAMPLER,
        keywords::trample::SCENARIOS,
    ),
    ("linked", names::CURRENCY_CONVERTER, linked::SCENARIOS),
    ("mana", names::MANA_DRAIN, mana::SCENARIOS),
    (
        "tokens",
        names::ADELINE_RESPLENDENT_CATHAR,
        tokens::ATTACKING,
    ),
    ("tokens", names::FRACTURED_IDENTITY, tokens::COPIES),
    (
        "zones",
        names::CURRENCY_CONVERTER,
        zones::PUT_INTO_GRAVEYARD,
    ),
    ("zones", names::ELITE_SPELLBINDER, zones::LOOK_AT),
];

/// The scenarios by the card that has the effect, as a card's own are listed
/// ([`crate::SCENARIOS`]).
pub fn by_card() -> impl Iterator<Item = (&'static str, &'static [Scenario])> {
    SCENARIOS
        .iter()
        .map(|&(_, card, scenarios)| (card, scenarios))
}
