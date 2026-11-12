//! The designated prop cards: the few cards besides basic lands that any card's scenarios may
//! name (plan, Verification: "Scenarios use basic lands and a small set of designated prop
//! cards, so one card's change cannot break another card's tests").
//!
//! Each plays one role scenarios need, and was chosen from the implemented pool (plan K14: no
//! new card for this) for text that stays out of a scenario's way: no upkeep trigger, no static
//! ability reaching other objects, no mana ability to add payment plans, nothing that happens
//! while it sits in a hand, a graveyard or a library. Mana Crypt, the old favourite, is the
//! counterexample: its upkeep coin flip asks a question of every scenario that passes a turn.
//!
//! A scenario about how its card meets another card in particular names that card and says so
//! with `Scenario::interaction`; `tests/props.rs` holds every other scenario to its own card,
//! that card's tokens, basic lands and these. When milestone 7b generates vanilla creatures
//! from the card store, they take over the creature roles.

use crate::names;

/// A creature to attack, block and be targeted: a 3/1 flier for {2}{W}. Its enters trigger
/// asks only when it enters.
pub const CREATURE: &str = names::ELITE_SPELLBINDER;

/// A second creature: a 2/1 for {1}{U}, a two-drop without flying, and a creature spell with
/// flash. Its enters trigger targets an instant or sorcery card in its controller's graveyard.
pub const SMALL_CREATURE: &str = names::SNAPCASTER_MAGE;

/// A noncreature artifact with no activated ability, and a cheap spell: {W}. Its enters trigger
/// exiles a nonland permanent with mana value 2 or less an opponent controls, so a scenario
/// that lets it resolve gives the opponent none, or means it.
pub const ARTIFACT: &str = names::PORTABLE_HOLE;

/// An instant: Counterspell, {U}{U}, to cast in response or to be countered.
pub const INSTANT: &str = names::COUNTERSPELL;

/// An instant that removes an opposing permanent or spell: Sink into Stupor, {1}{U}{U}, which
/// returns it to its owner's hand.
pub const REMOVAL: &str = names::SINK_INTO_STUPOR;

/// A sorcery that targets a creature: Oust, {W}.
pub const SORCERY: &str = names::OUST;

/// A spell that targets nothing and asks nothing: Timetwister, a {2}{U} sorcery.
pub const UNTARGETED_SPELL: &str = names::TIMETWISTER;

/// A planeswalker with no static or triggered ability: Jace, the Mind Sculptor, loyalty 3.
pub const PLANESWALKER: &str = names::JACE_THE_MIND_SCULPTOR;

/// A nonbasic land: Tundra, a Plains Island with no rules text.
pub const NONBASIC_LAND: &str = names::TUNDRA;

/// Every prop, for the check that scenarios name nothing else.
pub const ALL: &[&str] = &[
    CREATURE,
    SMALL_CREATURE,
    ARTIFACT,
    INSTANT,
    REMOVAL,
    SORCERY,
    UNTARGETED_SPELL,
    PLANESWALKER,
    NONBASIC_LAND,
];
