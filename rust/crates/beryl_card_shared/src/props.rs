//! The designated prop cards: the few cards besides basic lands that any card's scenarios may
//! name.
//!
//! Each plays one role scenarios need, chosen for text that stays out of a scenario's way. A
//! role no implemented card fills is filled by implementing a real card for it, even one in no
//! deck, so that `XMage` and Forge can play the same scenarios. A prop has no upkeep trigger, no
//! static ability reaching an object it isn't attached to, no mana ability to add payment
//! plans, nothing that happens
//! while it sits in a hand, a graveyard or a library. Mana Crypt, the old favourite, is the
//! counterexample: its upkeep coin flip asks a question of every scenario that passes a turn.
//!
//! A scenario about how its card meets another card in particular names that card and says so
//! with `Scenario::interaction`; `beryl_cards`' `tests/props.rs` holds every other scenario to
//! its own card, that card's tokens, basic lands and these, and every prop to a card of the
//! table. They are spelled out here, below the set crates, so that a card file names them
//! without the registry. When milestone 7b generates vanilla creatures from the card store,
//! they take over the creature roles.

/// A creature to attack, block and be targeted: a 3/1 flier for {2}{W}. Its enters trigger
/// asks only when it enters.
pub const CREATURE: &str = "Elite Spellbinder";

/// A second creature: a 2/1 for {1}{U}, a two-drop without flying, and a creature spell with
/// flash. Its enters trigger targets an instant or sorcery card in its controller's graveyard.
pub const SMALL_CREATURE: &str = "Snapcaster Mage";

/// A noncreature artifact with no activated ability, and a cheap spell: {W}. Its enters trigger
/// exiles a nonland permanent with mana value 2 or less an opponent controls, so a scenario
/// that lets it resolve gives the opponent none, or means it.
pub const ARTIFACT: &str = "Portable Hole";

/// An instant: Counterspell, {U}{U}, to cast in response or to be countered.
pub const INSTANT: &str = "Counterspell";

/// An instant that removes an opposing permanent or spell: Sink into Stupor, {1}{U}{U}, which
/// returns it to its owner's hand.
pub const REMOVAL: &str = "Sink into Stupor";

/// A sorcery that targets a creature: Oust, {W}.
pub const SORCERY: &str = "Oust";

/// A spell that targets nothing and asks nothing: Timetwister, a {2}{U} sorcery.
pub const UNTARGETED_SPELL: &str = "Timetwister";

/// A planeswalker with no static or triggered ability: Jace, the Mind Sculptor, loyalty 3.
pub const PLANESWALKER: &str = "Jace, the Mind Sculptor";

/// An Equipment: Leonin Scimitar, {1}, "Equipped creature gets +1/+1. Equip {1}". It is in no
/// deck: it was implemented for scenarios that need an Equipment.
pub const EQUIPMENT: &str = "Leonin Scimitar";

/// A nonbasic land: Tundra, a Plains Island with no rules text.
pub const NONBASIC_LAND: &str = "Tundra";

/// An instant that gives a creature a regeneration shield: Death Ward, {W}, "Regenerate
/// target creature." It is in no deck: it was implemented for generated scenarios in which a
/// destroy effect meets a shield.
pub const REGENERATION: &str = "Death Ward";

/// A creature that steals an artifact: Thieving Skydiver, {1}{U}, a 2/1 flier whose kicker
/// {X} gains control of target artifact with mana value X or less as it enters. Unkicked, its
/// trigger does nothing.
pub const THIEF: &str = "Thieving Skydiver";

/// Every prop, for the check that scenarios name nothing else.
pub const ALL: &[&str] = &[
    CREATURE,
    SMALL_CREATURE,
    ARTIFACT,
    EQUIPMENT,
    INSTANT,
    REMOVAL,
    SORCERY,
    UNTARGETED_SPELL,
    PLANESWALKER,
    NONBASIC_LAND,
    REGENERATION,
    THIEF,
];

/// The props as card files name them, `Prop::Creature`: the constants above, each with the name
/// of its role.
#[derive(Clone, Copy, Debug)]
pub struct Prop;

#[allow(non_upper_case_globals)]
impl Prop {
    /// [`CREATURE`].
    pub const Creature: &str = CREATURE;
    /// [`SMALL_CREATURE`].
    pub const SmallCreature: &str = SMALL_CREATURE;
    /// [`ARTIFACT`].
    pub const Artifact: &str = ARTIFACT;
    /// [`INSTANT`].
    pub const Instant: &str = INSTANT;
    /// [`REMOVAL`].
    pub const Removal: &str = REMOVAL;
    /// [`SORCERY`].
    pub const Sorcery: &str = SORCERY;
    /// [`UNTARGETED_SPELL`].
    pub const UntargetedSpell: &str = UNTARGETED_SPELL;
    /// [`PLANESWALKER`].
    pub const Planeswalker: &str = PLANESWALKER;
    /// [`EQUIPMENT`].
    pub const Equipment: &str = EQUIPMENT;
    /// [`NONBASIC_LAND`].
    pub const NonbasicLand: &str = NONBASIC_LAND;
    /// [`REGENERATION`].
    pub const Regeneration: &str = REGENERATION;
    /// [`THIEF`].
    pub const Thief: &str = THIEF;
}
