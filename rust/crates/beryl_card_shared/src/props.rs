//! The designated prop cards: the few cards besides basic lands that any card's scenarios may
//! name, each standing in for "a creature", "an instant", "an Equipment" and so on.
//!
//! A scenario reads `Prop::Flier`, not "Geist of the Moors", so that it says which role the
//! card plays and nothing else. For that to work, everything a scenario may rely on about a prop
//! is in its [`Contract`] below: its mana cost, its card types, its power and toughness or
//! loyalty, its keywords and a summary of the rest of its rules text. `beryl_cards`'
//! `tests/props.rs` checks each contract against the card's definition, so a contract can't
//! drift from its card, and a prop can be replaced by another card with the same contract
//! without touching any scenario. A scenario never says which card a prop is, and never
//! explains a prop's stats in a comment: a reader finds them here.
//!
//! Each prop is a real card, so that `XMage` and Forge can play the same scenarios, and the
//! creature props have no rules text beyond their keywords, so that nothing they do gets in a
//! scenario's way. The other props have the one ability their role needs and nothing that
//! happens on its own: no upkeep trigger, no static ability reaching an object it isn't attached
//! to, no mana ability to add payment plans, nothing that happens while it sits in a hand, a
//! graveyard or a library. Mana Crypt, the old favourite, is the counterexample: its upkeep coin
//! flip asks a question of every scenario that passes a turn.
//!
//! A role no prop fills is filled by implementing a real card for it, even one in no deck, and
//! adding a contract here when other cards' scenarios will need the same role.
//!
//! A scenario about how its card meets another card in particular names that card and says so
//! with `Scenario::interaction`; `beryl_cards`' `tests/props.rs` holds every other scenario to
//! its own card, that card's tokens, basic lands and these. They are spelled out here, below the
//! set crates, so that a card file names them without the registry.

use beryl_card::{Keywords, ManaCost, Subtype, Subtypes, Types};

/// What a scenario may rely on about a prop. Anything not written here may change when the
/// prop's card is replaced.
#[derive(Clone, Copy, Debug)]
pub struct Contract {
    /// The card that fills the role today. Only this module and the test of the contracts
    /// write it.
    pub card: &'static str,
    /// Its mana cost, and through it its colors and mana value.
    pub cost: ManaCost,
    /// Its card types.
    pub types: Types,
    /// Its subtypes, such as its creature types.
    pub subtypes: Subtypes,
    /// Its power and toughness, or its loyalty.
    pub stats: Stats,
    /// Its keyword abilities.
    pub keywords: Keywords,
    /// Its rules text other than keywords, in brief, or `""` when it has none.
    pub text: &'static str,
}

/// A prop's power and toughness or starting loyalty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stats {
    None,
    Creature { power: i8, toughness: i8 },
    Planeswalker { loyalty: u8 },
}

/// Mana symbols by color, in [`ManaCost::symbols`] order: W, U, B, R, G, C.
const fn cost(generic: u8, white: u8, blue: u8, black: u8) -> ManaCost {
    ManaCost::new(generic, [white, blue, black, 0, 0, 0])
}

/// A creature to attack, block and be targeted: a white 3/1 flier for {1}{W}{W}, mana value 3,
/// a Spirit.
pub const FLIER: Contract = Contract {
    card: "Geist of the Moors",
    cost: cost(1, 2, 0, 0),
    types: Types::CREATURE,
    subtypes: Subtypes::of(&[Subtype::Spirit]),
    stats: Stats::Creature {
        power: 3,
        toughness: 1,
    },
    keywords: Keywords::FLYING,
    text: "",
};

/// A creature without flying: a blue 2/1 for {1}{U}, mana value 2, an Elk.
pub const VANILLA: Contract = Contract {
    card: "Wetland Sambar",
    cost: cost(1, 0, 1, 0),
    types: Types::CREATURE,
    subtypes: Subtypes::of(&[Subtype::Elk]),
    stats: Stats::Creature {
        power: 2,
        toughness: 1,
    },
    keywords: Keywords::NONE,
    text: "",
};

/// A creature spell to cast at instant speed: a black 3/2 with flash for {2}{B}, mana value
/// 3, a Human Assassin.
pub const FLASH_CREATURE: Contract = Contract {
    card: "Hired Blade",
    cost: cost(2, 0, 0, 1),
    types: Types::CREATURE,
    subtypes: Subtypes::of(&[Subtype::Human, Subtype::Assassin]),
    stats: Stats::Creature {
        power: 3,
        toughness: 2,
    },
    keywords: Keywords::FLASH,
    text: "",
};

/// A noncreature artifact with no activated ability, and a cheap spell: a white artifact for
/// {W}. Its enters trigger exiles a nonland permanent with mana value 2 or less an opponent
/// controls, so a scenario that lets it resolve gives the opponent none, or means it.
pub const ARTIFACT: Contract = Contract {
    card: "Portable Hole",
    cost: cost(0, 1, 0, 0),
    types: Types::ARTIFACT,
    subtypes: Subtypes::NONE,
    stats: Stats::None,
    keywords: Keywords::NONE,
    text: "When it enters, exile target nonland permanent an opponent controls with mana value 2 \
           or less until it leaves the battlefield.",
};

/// An instant to cast in response or to be countered: {U}{U}, "Counter target spell."
pub const INSTANT: Contract = Contract {
    card: "Counterspell",
    cost: cost(0, 0, 2, 0),
    types: Types::INSTANT,
    subtypes: Subtypes::NONE,
    stats: Stats::None,
    keywords: Keywords::NONE,
    text: "Counter target spell.",
};

/// An instant that removes an opposing permanent or spell: {1}{U}{U}, returning it to its
/// owner's hand.
pub const REMOVAL: Contract = Contract {
    card: "Sink into Stupor",
    cost: cost(1, 0, 2, 0),
    types: Types::INSTANT,
    subtypes: Subtypes::NONE,
    stats: Stats::None,
    keywords: Keywords::NONE,
    text: "Return target spell or nonland permanent an opponent controls to its owner's hand. \
           Its back face is a land.",
};

/// A sorcery that targets a creature: {W}, which puts the creature into its owner's library
/// second from the top, and its controller gains 3 life.
pub const SORCERY: Contract = Contract {
    card: "Oust",
    cost: cost(0, 1, 0, 0),
    types: Types::SORCERY,
    subtypes: Subtypes::NONE,
    stats: Stats::None,
    keywords: Keywords::NONE,
    text: "Put target creature into its owner's library second from the top. Its controller \
           gains 3 life.",
};

/// A spell that targets nothing and asks nothing: a {2}{U} sorcery that shuffles each player's
/// hand and graveyard into their library, after which each draws seven cards.
pub const UNTARGETED_SPELL: Contract = Contract {
    card: "Timetwister",
    cost: cost(2, 0, 1, 0),
    types: Types::SORCERY,
    subtypes: Subtypes::NONE,
    stats: Stats::None,
    keywords: Keywords::NONE,
    text: "Each player shuffles their hand and graveyard into their library, then draws seven \
           cards.",
};

/// A planeswalker with no static or triggered ability: {2}{U}{U}, loyalty 3.
pub const PLANESWALKER: Contract = Contract {
    card: "Jace, the Mind Sculptor",
    cost: cost(2, 0, 2, 0),
    types: Types::PLANESWALKER,
    subtypes: Subtypes::of(&[Subtype::Jace]),
    stats: Stats::Planeswalker { loyalty: 3 },
    keywords: Keywords::NONE,
    text: "Four loyalty abilities: +2 look at the top card of a library; 0 draw three, put back \
           two; -1 bounce a creature; -12 exile a library and shuffle a hand into it.",
};

/// An Equipment: {1}, "Equipped creature gets +1/+1. Equip {1}". It is in no deck: it was
/// implemented for scenarios that need an Equipment.
pub const EQUIPMENT: Contract = Contract {
    card: "Leonin Scimitar",
    cost: cost(1, 0, 0, 0),
    types: Types::ARTIFACT,
    subtypes: Subtypes::of(&[Subtype::Equipment]),
    stats: Stats::None,
    keywords: Keywords::NONE,
    text: "Equipped creature gets +1/+1. Equip {1}.",
};

/// A nonbasic land: a Plains Island, which taps for {W} or {U} from its land types and has no other rules text.
pub const NONBASIC_LAND: Contract = Contract {
    card: "Tundra",
    cost: ManaCost::NONE,
    types: Types::LAND,
    subtypes: Subtypes::of(&[Subtype::Plains, Subtype::Island]),
    stats: Stats::None,
    keywords: Keywords::NONE,
    text: "{T}: Add {W} or {U}, from its land types.",
};

/// An instant that gives a creature a regeneration shield: {W}, "Regenerate target creature."
/// It is in no deck: it was implemented for generated scenarios in which a destroy effect
/// meets a shield.
pub const REGENERATION: Contract = Contract {
    card: "Death Ward",
    cost: cost(0, 1, 0, 0),
    types: Types::INSTANT,
    subtypes: Subtypes::NONE,
    stats: Stats::None,
    keywords: Keywords::NONE,
    text: "Regenerate target creature.",
};

/// A creature that steals an artifact: a blue 2/1 flier for {1}{U}, a Merfolk Rogue, whose kicker {X} gains
/// control of target artifact with mana value X or less as it enters. Unkicked, its trigger
/// does nothing.
pub const THIEF: Contract = Contract {
    card: "Thieving Skydiver",
    cost: cost(1, 0, 1, 0),
    types: Types::CREATURE,
    subtypes: Subtypes::of(&[Subtype::Merfolk, Subtype::Rogue]),
    stats: Stats::Creature {
        power: 2,
        toughness: 1,
    },
    keywords: Keywords::FLYING,
    text: "Kicker {X}. When it enters, if it was kicked, gain control of target artifact with \
           mana value X or less, untapping it and attaching it to this creature if it's an \
           Equipment.",
};

/// Every prop, for the check that scenarios name nothing else and the check of the contracts.
pub const ALL: &[Contract] = &[
    FLIER,
    VANILLA,
    FLASH_CREATURE,
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

/// The props as card files name them, `Prop::Flier`: each contract's card, by the name of its
/// role.
#[derive(Clone, Copy, Debug)]
pub struct Prop;

#[allow(non_upper_case_globals)]
impl Prop {
    /// [`FLIER`].
    pub const Flier: &str = FLIER.card;
    /// [`VANILLA`].
    pub const Vanilla: &str = VANILLA.card;
    /// [`FLASH_CREATURE`].
    pub const FlashCreature: &str = FLASH_CREATURE.card;
    /// [`ARTIFACT`].
    pub const Artifact: &str = ARTIFACT.card;
    /// [`INSTANT`].
    pub const Instant: &str = INSTANT.card;
    /// [`REMOVAL`].
    pub const Removal: &str = REMOVAL.card;
    /// [`SORCERY`].
    pub const Sorcery: &str = SORCERY.card;
    /// [`UNTARGETED_SPELL`].
    pub const UntargetedSpell: &str = UNTARGETED_SPELL.card;
    /// [`PLANESWALKER`].
    pub const Planeswalker: &str = PLANESWALKER.card;
    /// [`EQUIPMENT`].
    pub const Equipment: &str = EQUIPMENT.card;
    /// [`NONBASIC_LAND`].
    pub const NonbasicLand: &str = NONBASIC_LAND.card;
    /// [`REGENERATION`].
    pub const Regeneration: &str = REGENERATION.card;
    /// [`THIEF`].
    pub const Thief: &str = THIEF.card;
}
