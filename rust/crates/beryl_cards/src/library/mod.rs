//! Hand-written scenarios of the rules library's entries, in a tree that mirrors the library's modules. Rules code names no card and
//! the oracles play real ones, so an entry's scenario lives here: it uses basic lands, the
//! designated props ([`crate::props`]) and one card that has the effect.

use beryl_scenario::Scenario;

use crate::{names, props};

pub mod actions;
pub mod casting;
pub mod choices;
pub mod coins;
pub mod combat;
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
pub mod replacement;
pub mod tokens;
pub mod turn;
pub mod zones;

/// Every entry's scenarios: the library module, the card that has the effect, and the
/// scenarios, which the oracles play as that card's.
pub static SCENARIOS: &[(&str, &str, &[Scenario])] = &[
    (
        "actions::counter",
        names::MANA_TITHE,
        actions::counter::UNLESS_PAID,
    ),
    (
        "actions::counter",
        names::NO_MORE_LIES,
        actions::counter::SCENARIOS,
    ),
    (
        "actions::counter",
        names::COUNTERSPELL,
        actions::counter::GRAVEYARD,
    ),
    (
        "actions::explore",
        names::GET_LOST,
        actions::explore::SCENARIOS,
    ),
    (
        "actions::fateseal",
        names::JACE_THE_MIND_SCULPTOR,
        actions::fateseal::TOP_OR_BOTTOM,
    ),
    (
        "actions::fateseal",
        names::JACE_THE_MIND_SCULPTOR,
        actions::fateseal::SCENARIOS,
    ),
    (
        "actions::search",
        names::TRINKET_MAGE,
        actions::search::FIND_AND_REVEAL,
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
    ("combat", props::VANILLA.card, combat::DECLARATIONS),
    ("combat", names::WHITE_KNIGHT, combat::FIRST_STRIKE),
    ("combat", names::GET_LOST, combat::REMOVED_BLOCKER),
    ("combat", names::GET_LOST, combat::REMOVED_PLANESWALKER),
    ("combat", names::BLOODGHAST, combat::CANNOT_BLOCK),
    (
        "continuous",
        names::TEFERI_TIME_RAVELER,
        continuous::SCENARIOS,
    ),
    (
        "continuous",
        names::OPALESCENCE,
        continuous::ANIMATED_ENCHANTMENTS,
    ),
    (
        "continuous",
        names::BLOOD_MOON,
        continuous::SETTING_LAND_TYPES,
    ),
    ("continuous", names::DRYAD_ARBOR, continuous::LAND_CREATURE),
    (
        "continuous",
        names::VIGILANCE,
        continuous::ABILITIES_IN_TIMESTAMP_ORDER,
    ),
    ("costs", names::THIRST_FOR_DISCOVERY, costs::SCENARIOS),
    (
        "costs",
        names::THRILL_OF_POSSIBILITY,
        costs::DISCARD_TO_CAST,
    ),
    ("costs", names::VILLAGE_RITES, costs::SACRIFICE_TO_CAST),
    ("costs", names::SPARAS_HEADQUARTERS, costs::CYCLING),
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
    ("library", names::OUST, library::SECOND_FROM_TOP),
    (
        "library",
        names::JACE_THE_MIND_SCULPTOR,
        library::PUT_BACK_IN_ORDER,
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
        keywords::flashback::INSTANT_RESPONSE,
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
        "replacement",
        names::CLEVER_IMPERSONATOR,
        replacement::COPIES_TRANSFORMED_PERMANENT,
    ),
    (
        "replacement",
        names::DARK_DEPTHS,
        replacement::ENTERS_WITH_COUNTERS,
    ),
    (
        "tokens",
        names::ADELINE_RESPLENDENT_CATHAR,
        tokens::ATTACKING,
    ),
    ("tokens", names::FRACTURED_IDENTITY, tokens::COPIES),
    ("turn", names::MINDSLAVER, turn::CONTROLLED_TURN),
    ("turn", names::ISLAND, turn::FIRST_DRAW),
    ("turn", names::ISLAND, turn::LAND_PLAY),
    ("turn", names::PLAINS, turn::CLEANUP),
    (
        "zones",
        names::CURRENCY_CONVERTER,
        zones::PUT_INTO_GRAVEYARD,
    ),
    ("zones", names::BLOODGHAST, zones::RETURN_FROM_GRAVEYARD),
    ("zones", names::BLOODGHAST, zones::GRAVEYARD_RETURN),
    ("zones", names::ELITE_SPELLBINDER, zones::LOOK_AT),
    (
        "zones",
        names::SINK_INTO_STUPOR,
        zones::DECLINE_LIFE_TO_ENTER_UNTAPPED,
    ),
];

/// The scenarios by the card that has the effect, as a card's own are listed
/// ([`crate::SCENARIOS`]).
pub fn by_card() -> impl Iterator<Item = (&'static str, &'static [Scenario])> {
    SCENARIOS
        .iter()
        .map(|&(_, card, scenarios)| (card, scenarios))
}
