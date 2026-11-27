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
pub mod shared;
pub mod tokens;
pub mod triggers;
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
        "actions::counter",
        names::COUNTERSPELL,
        actions::counter::SELF_TARGET,
    ),
    (
        "actions::counter",
        names::COUNTERSPELL,
        actions::counter::SAME_NAME,
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
    (
        "casting",
        crate::props::VANILLA.card,
        casting::SORCERY_TIMING,
    ),
    (
        "casting",
        names::TEFERI_TIME_RAVELER,
        casting::LOYALTY_TIMING,
    ),
    ("casting", names::GET_LOST, casting::LEGAL_TARGETS),
    ("casting", names::THIEVING_SKYDIVER, casting::KICKER_X),
    ("casting", names::THUNDERTRAP_TRAINER, casting::OFFSPRING),
    (
        "casting",
        names::OVERLORD_OF_THE_MISTMOORS,
        casting::IMPENDING,
    ),
    ("casting", names::STRIP_MINE, casting::SACRIFICED_SOURCE),
    (
        "casting",
        crate::props::ARTIFACT.card,
        casting::TRIGGER_TARGETS,
    ),
    ("casting", names::TENDRILS_OF_AGONY, casting::STORM),
    ("choices", names::ELITE_SPELLBINDER, choices::SCENARIOS),
    ("coins", names::MANA_CRYPT, coins::SCENARIOS),
    ("combat", props::VANILLA.card, combat::DECLARATIONS),
    ("combat", names::WHITE_KNIGHT, combat::FIRST_STRIKE),
    ("combat", names::FENCING_ACE, combat::DOUBLE_STRIKE),
    ("combat", names::GET_LOST, combat::REMOVED_BLOCKER),
    ("combat", names::GET_LOST, combat::REMOVED_PLANESWALKER),
    ("combat", names::BLOODGHAST, combat::CANNOT_BLOCK),
    ("combat", names::LURE, combat::MUST_BLOCK),
    ("combat", names::ALLEY_STRANGLER, combat::MENACE),
    ("combat", names::BLOODROCK_CYCLOPS, combat::MUST_ATTACK),
    ("combat", names::PROPAGANDA, combat::ATTACK_COST),
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
    ("continuous", names::GLORIOUS_ANTHEM, continuous::ANTHEM),
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
    ("costs", names::CHORD_OF_CALLING, costs::CONVOKE),
    ("damage", names::MANA_CRYPT, damage::SCENARIOS),
    ("damage", names::LIGHTNING_BOLT, damage::ANY_TARGET),
    (
        "delayed_triggers",
        names::TEFERI_HERO_OF_DOMINARIA,
        delayed_triggers::SCENARIOS,
    ),
    (
        "delayed_triggers",
        names::MANA_DRAIN,
        delayed_triggers::NEXT_MAIN_PHASE,
    ),
    (
        "delayed_triggers",
        names::FLICKERWISP,
        delayed_triggers::RETURN_TO_OWNER,
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
        "keywords::banding",
        names::BENALISH_HERO,
        keywords::banding::HERO_SCENARIOS,
    ),
    (
        "keywords::banding",
        names::BENALISH_INFANTRY,
        keywords::banding::INFANTRY_SCENARIOS,
    ),
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
    (
        "keywords::flashback",
        names::SNAPCASTER_MAGE,
        keywords::flashback::RETURN_TO_HAND,
    ),
    ("linked", names::CURRENCY_CONVERTER, linked::SCENARIOS),
    ("mana", names::MANA_DRAIN, mana::SCENARIOS),
    ("mana", names::MANA_CRYPT, mana::FLOATING_MANA),
    ("mana", crate::props::VANILLA.card, mana::EQUIVALENT_SOURCES),
    ("mana", names::AZORIUS_SIGNET, mana::PRICED_MANA),
    ("mana", names::HIGH_TIDE, mana::HIGH_TIDE),
    ("mana", names::MANA_FLARE, mana::MANA_FLARE),
    ("mana", names::WILD_GROWTH, mana::WILD_GROWTH),
    ("mana", names::MANA_REFLECTION, mana::MANA_REFLECTION),
    ("mana", names::REFLECTING_POOL, mana::REFLECTING_POOL),
    ("mana", names::GEMHIDE_SLIVER, mana::GRANTED_MANA),
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
        "replacement",
        names::DOUBLING_SEASON,
        replacement::DOUBLED_COUNTERS,
    ),
    (
        "replacement",
        names::HARDENED_SCALES,
        replacement::PLUS_ONE_COUNTER,
    ),
    (
        "tokens",
        names::ADELINE_RESPLENDENT_CATHAR,
        tokens::ATTACKING,
    ),
    ("tokens", names::FRACTURED_IDENTITY, tokens::COPIES),
    ("tokens", names::PARALLEL_LIVES, tokens::DOUBLED),
    ("triggers", names::THIEVING_SKYDIVER, triggers::X_FROM_SPELL),
    (
        "triggers",
        names::OVERLORD_OF_THE_MISTMOORS,
        triggers::CONTROLLERS_END_STEP,
    ),
    ("triggers", names::DRESS_DOWN, triggers::OPPONENTS_STEP),
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
    (
        "zones",
        names::SINK_INTO_STUPOR,
        zones::PAY_LIFE_TO_ENTER_UNTAPPED,
    ),
    ("zones", names::SPARAS_HEADQUARTERS, zones::ENTERS_TAPPED),
    ("shared", names::ISLAND, shared::DRAW),
    ("shared", names::MENTAL_NOTE, shared::MILL),
    ("shared", names::OUST, shared::RETURN_TO_TOP),
    ("shared", names::TIMETWISTER, shared::SHUFFLE),
    ("shared", names::TIMETWISTER, shared::SIMULTANEOUS_DRAW),
    ("shared", names::ISLAND, shared::FREE_MULLIGAN),
    ("shared", names::ISLAND, shared::DECKING),
    ("shared", names::SNAPCASTER_MAGE, shared::FLASHBACK),
    ("shared", names::ISLAND, shared::ISLAND),
];

/// The scenarios by the card that has the effect, as a card's own are listed
/// ([`crate::SCENARIOS`]).
pub fn by_card() -> impl Iterator<Item = (&'static str, &'static [Scenario])> {
    SCENARIOS
        .iter()
        .map(|&(_, card, scenarios)| (card, scenarios))
}
