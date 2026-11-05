//! Card definitions: typed data for everything the engine inspects, Rust functions for what an
//! ability does (architecture §6.1, §6.2, decision D1).
//!
//! Every type here is `'static` table data shared by all threads. `State` never holds one;
//! it stores game-table indices (§6.4) and ability indices instead.

use crate::effect::EffectCtx;
use crate::filter::{Filter, Whose};
use crate::state::{CounterKind, PendingTrigger, State, TurnStep};
use crate::table::GameTable;

/// One card: its name and its faces.
#[derive(Debug)]
pub struct CardDef {
    pub name: &'static str,
    /// One face, or two for double-faced, modal double-faced, split and adventure cards.
    pub faces: &'static [Face],
    /// The tokens and emblems this card can create, so a game's table holds them (§6.4).
    pub tokens: &'static [&'static CardDef],
}

impl CardDef {
    /// The face a card has in every zone but the battlefield and the stack, where it may have
    /// another one up.
    pub fn front(&self) -> &'static Face {
        &self.faces[0]
    }
}

/// The characteristics printed on one face.
#[derive(Debug)]
pub struct Face {
    pub cost: ManaCost,
    pub supertypes: Supertypes,
    pub types: Types,
    pub subtypes: Subtypes,
    pub colors: Colors,
    pub power: Option<i8>,
    pub toughness: Option<i8>,
    pub loyalty: Option<u8>,
    pub keywords: Keywords,
    pub abilities: &'static [Ability],
}

impl Face {
    /// A face with no cost, types or abilities, for `..Face::EMPTY` in card files.
    pub const EMPTY: Face = Face {
        cost: ManaCost::NONE,
        supertypes: Supertypes::NONE,
        types: Types::NONE,
        subtypes: Subtypes::NONE,
        colors: Colors::NONE,
        power: None,
        toughness: None,
        loyalty: None,
        keywords: Keywords::NONE,
        abilities: &[],
    };
}

/// A mana cost: generic mana plus colored and colorless symbols.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ManaCost {
    pub generic: u8,
    /// Symbols by [`ManaColor`] index: W, U, B, R, G, C.
    pub symbols: [u8; 6],
}

impl ManaCost {
    pub const NONE: ManaCost = ManaCost {
        generic: 0,
        symbols: [0; 6],
    };

    /// The mana value ([CR 202.3](https://yawgatog.com/resources/magic-rules/#R2023)): "The
    /// mana value of an object is a number equal to the total amount of mana in its mana cost,
    /// regardless of color."
    pub fn mana_value(self) -> u8 {
        self.generic + self.symbols.iter().sum::<u8>()
    }
}

/// A kind of mana: the five colors and colorless.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ManaColor {
    White,
    Blue,
    Black,
    Red,
    Green,
    Colorless,
}

/// What a mana ability can produce: a set of [`ManaColor`]s and how much.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ManaSet {
    /// A bit per [`ManaColor`].
    pub colors: u8,
    pub amount: u8,
}

impl ManaSet {
    pub const NONE: ManaSet = ManaSet {
        colors: 0,
        amount: 0,
    };

    /// One mana of one kind.
    pub const fn one(color: ManaColor) -> ManaSet {
        ManaSet {
            colors: 1 << color as u8,
            amount: 1,
        }
    }

    pub fn includes(self, color: ManaColor) -> bool {
        self.colors & (1 << color as u8) != 0
    }
}

macro_rules! bitset {
    ($(#[$meta:meta])* $name:ident($repr:ty) { $($(#[$fmeta:meta])* $flag:ident = $bit:expr),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
        pub struct $name(pub $repr);

        impl $name {
            pub const NONE: $name = $name(0);
            $($(#[$fmeta])* pub const $flag: $name = $name(1 << $bit);)*

            #[must_use]
            pub const fn union(self, other: $name) -> $name {
                $name(self.0 | other.0)
            }

            pub fn contains(self, other: $name) -> bool {
                self.0 & other.0 == other.0
            }

            pub fn intersects(self, other: $name) -> bool {
                self.0 & other.0 != 0
            }
        }
    };
}

bitset! {
    /// Card types ([CR 300.1](https://yawgatog.com/resources/magic-rules/#R3001)), those the
    /// cube's cards use.
    Types(u8) {
        ARTIFACT = 0,
        CREATURE = 1,
        ENCHANTMENT = 2,
        INSTANT = 3,
        LAND = 4,
        PLANESWALKER = 5,
        SORCERY = 6,
    }
}

bitset! {
    /// Supertypes ([CR 205.4a](https://yawgatog.com/resources/magic-rules/#R2054a)), those the
    /// cube's cards use.
    Supertypes(u8) {
        BASIC = 0,
        LEGENDARY = 1,
    }
}

bitset! {
    /// Colors ([CR 105.1](https://yawgatog.com/resources/magic-rules/#R1051)): "There are five
    /// colors in the Magic game: white, blue, black, red, and green."
    Colors(u8) {
        WHITE = 0,
        BLUE = 1,
        BLACK = 2,
        RED = 3,
        GREEN = 4,
    }
}

bitset! {
    /// Keyword abilities, one bit each. The cards that need them add them.
    Keywords(u64) {
        /// [CR 702.7a](https://yawgatog.com/resources/magic-rules/#R7027a): "First strike is
        /// a static ability that modifies the rules for the combat damage step."
        FIRST_STRIKE = 0,
        /// [CR 702.9b](https://yawgatog.com/resources/magic-rules/#R7029b): "A creature with
        /// flying can't be blocked except by creatures with flying and/or reach."
        FLYING = 8,
        /// [CR 702.17b](https://yawgatog.com/resources/magic-rules/#R70217b): "A creature
        /// with flying can't be blocked except by creatures with flying and/or reach."
        REACH = 9,
        /// [CR 702.20b](https://yawgatog.com/resources/magic-rules/#R70220b): "Attacking
        /// doesn't cause creatures with vigilance to tap."
        VIGILANCE = 10,
    }
}

/// A subtype ([CR 205.3](https://yawgatog.com/resources/magic-rules/#R2053)). The cards that
/// need more add them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Subtype {
    // The basic land types (CR 305.6).
    Plains,
    Island,
    Swamp,
    Mountain,
    Forest,
    /// An artifact type ([CR 301.7](https://yawgatog.com/resources/magic-rules/#R3017)).
    Vehicle,
    /// An enchantment type ([CR 303.4](https://yawgatog.com/resources/magic-rules/#R3034)):
    /// "Some enchantments have the subtype "Aura." An Aura enters the battlefield attached to
    /// an object or player."
    Aura,
}

impl Subtype {
    pub const ALL: [Subtype; 7] = [
        Subtype::Plains,
        Subtype::Island,
        Subtype::Swamp,
        Subtype::Mountain,
        Subtype::Forest,
        Subtype::Vehicle,
        Subtype::Aura,
    ];

    /// The card type this subtype belongs to (CR 205.3).
    pub const fn card_type(self) -> Types {
        match self {
            Subtype::Plains
            | Subtype::Island
            | Subtype::Swamp
            | Subtype::Mountain
            | Subtype::Forest => Types::LAND,
            Subtype::Vehicle => Types::ARTIFACT,
            Subtype::Aura => Types::ENCHANTMENT,
        }
    }
}

/// A set of [`Subtype`]s, one bit each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Subtypes(pub [u64; 2]);

impl Subtypes {
    pub const NONE: Subtypes = Subtypes([0; 2]);

    pub const fn of(subtypes: &[Subtype]) -> Subtypes {
        let mut bits = [0; 2];
        let mut i = 0;
        while i < subtypes.len() {
            let s = subtypes[i] as u8;
            bits[(s >> 6) as usize] |= 1 << (s & 63);
            i += 1;
        }
        Subtypes(bits)
    }

    pub fn contains(self, subtype: Subtype) -> bool {
        let s = subtype as u8;
        self.0[usize::from(s >> 6)] & (1 << (s & 63)) != 0
    }

    #[must_use]
    pub fn with(self, subtype: Subtype) -> Subtypes {
        let mut bits = self.0;
        let s = subtype as u8;
        bits[usize::from(s >> 6)] |= 1 << (s & 63);
        Subtypes(bits)
    }

    /// The subtypes in the set.
    pub fn iter(self) -> impl Iterator<Item = Subtype> {
        Subtype::ALL.into_iter().filter(move |&s| self.contains(s))
    }
}

/// What kind of ability this is ([CR 113.3](https://yawgatog.com/resources/magic-rules/#R1133)):
/// "There are four general categories of abilities": spell, activated, triggered and static.
/// Mana abilities ([CR 605](https://yawgatog.com/resources/magic-rules/#R605)) are activated
/// or triggered abilities with special rules, so they are a kind of their own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AbilityKind {
    Spell,
    Activated,
    Mana,
    Triggered,
    Static,
    /// A static ability that creates a replacement effect (CR 614).
    Replacement,
}

/// An activation or casting cost beyond the mana cost.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Cost {
    pub mana: ManaCost,
    /// The tap symbol ([CR 107.5](https://yawgatog.com/resources/magic-rules/#R1075)): "The tap
    /// symbol in an activation cost means 'Tap this permanent.'"
    pub tap: bool,
}

impl Cost {
    pub const NONE: Cost = Cost {
        mana: ManaCost::NONE,
        tap: false,
    };
    pub const TAP: Cost = Cost {
        mana: ManaCost::NONE,
        tap: true,
    };
}

/// When an ability may be activated or a spell cast.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Timing {
    /// Any time its controller has priority
    /// ([CR 117.1a](https://yawgatog.com/resources/magic-rules/#R1171a)): "A player may cast an
    /// instant spell any time they have priority."
    Instant,
    /// Only during its controller's main phase while the stack is empty
    /// ([CR 307.1](https://yawgatog.com/resources/magic-rules/#R3071)).
    Sorcery,
}

/// One target an ability or spell requires.
#[derive(Clone, Copy, Debug)]
pub struct TargetSpec {
    pub filter: Filter,
    pub count: u8,
    pub optional: bool,
}

/// One ability of a face: its interface as data, and what it does as a Rust function.
#[derive(Debug)]
pub struct Ability {
    pub kind: AbilityKind,
    pub cost: Cost,
    pub timing: Timing,
    pub targets: &'static [TargetSpec],
    /// Mana abilities: what they produce.
    pub produces: ManaSet,
    /// Whether the body may ask its controller a question mid-resolution (§7.3).
    pub may_pause: bool,
    pub body: fn(&mut EffectCtx) -> Flow,
    /// Triggered abilities: the event they trigger on (§8.3).
    pub trigger: Trigger,
    /// Triggered abilities: an intervening "if" clause, checked when the ability triggers
    /// and again as it resolves
    /// ([CR 603.4](https://yawgatog.com/resources/magic-rules/#R6034)): "The ability triggers
    /// only if it is; otherwise it does nothing. If the ability triggers, it checks the
    /// stated condition again as it resolves."
    pub condition: Option<fn(&State, &GameTable, &PendingTrigger) -> bool>,
    /// Replacement abilities: the event they modify.
    pub replacement: Replacement,
    /// Static abilities: the continuous effect they generate on the battlefield (§9).
    pub layer: Option<LayerSpec>,
}

impl Ability {
    /// An ability with no cost, interface or effect, for `..Ability::NONE` in card files.
    pub const NONE: Ability = Ability {
        kind: AbilityKind::Static,
        cost: Cost::NONE,
        timing: Timing::Instant,
        targets: &[],
        produces: ManaSet::NONE,
        may_pause: false,
        body: crate::effect::nothing,
        trigger: Trigger::None,
        condition: None,
        replacement: Replacement::None,
        layer: None,
    };

    /// A triggered ability.
    pub const fn triggered(trigger: Trigger, body: fn(&mut EffectCtx) -> Flow) -> Ability {
        Ability {
            kind: AbilityKind::Triggered,
            trigger,
            body,
            ..Ability::NONE
        }
    }

    /// A static ability generating a continuous effect.
    pub const fn statik(layer: LayerSpec) -> Ability {
        Ability {
            layer: Some(layer),
            ..Ability::NONE
        }
    }

    /// A static ability creating a replacement effect.
    pub const fn replacing(replacement: Replacement) -> Ability {
        Ability {
            kind: AbilityKind::Replacement,
            replacement,
            ..Ability::NONE
        }
    }

    /// A mana ability with the cost {T}, such as the intrinsic ability of a basic land type
    /// ([CR 305.6](https://yawgatog.com/resources/magic-rules/#R3056)): "An object with the
    /// land card type and a basic land type has the intrinsic ability '{T}: Add [mana
    /// symbol],' even if the text box doesn't actually contain that text."
    pub const fn tap_for_mana(produces: ManaSet) -> Ability {
        Ability {
            kind: AbilityKind::Mana,
            cost: Cost::TAP,
            produces,
            body: crate::effect::add_produced_mana,
            ..Ability::NONE
        }
    }
}

/// What a triggered ability triggers on
/// ([CR 603.2](https://yawgatog.com/resources/magic-rules/#R6032)): "Whenever a game event
/// or game state matches a triggered ability's trigger event, that ability automatically
/// triggers." The kinds the cube's cards use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Trigger {
    None,
    /// "When this enters"
    /// ([CR 603.6a](https://yawgatog.com/resources/magic-rules/#R6036a)).
    Enters,
    /// "When this dies": a leaves-the-battlefield ability, which looks back in time
    /// ([CR 603.10a](https://yawgatog.com/resources/magic-rules/#R60310a)).
    Dies,
    /// "Whenever this attacks"
    /// ([CR 508.3a](https://yawgatog.com/resources/magic-rules/#R5083a)): it "triggers if
    /// that creature is declared as an attacker."
    Attacks,
    /// "Whenever you attack"
    /// ([CR 508.3d](https://yawgatog.com/resources/magic-rules/#R5083d)): it "triggers if
    /// one or more creatures that player controls are declared as attackers."
    YouAttack,
    /// "Whenever this deals combat damage to a player".
    CombatDamageToPlayer,
    /// "At the beginning of [whose] [step]"
    /// ([CR 500.6](https://yawgatog.com/resources/magic-rules/#R5006)): "When a phase or
    /// step begins, any abilities that trigger 'at the beginning of' that phase or step
    /// trigger."
    Step(TurnStep, Whose),
    /// "Whenever you create one or more tokens" of these types: once per event, however
    /// many tokens it creates.
    YouCreateTokens(Types),
    /// "Whenever you cast a spell".
    YouCast,
    /// A delayed triggered ability (CR 603.7): never matched against events, but created by
    /// a body with the moment it waits for.
    Delayed,
}

/// The moment a delayed triggered ability waits for. It triggers once, the next time that
/// moment comes ([CR 603.7b](https://yawgatog.com/resources/magic-rules/#R6037b)): "A
/// delayed triggered ability will trigger only once--the next time its trigger event
/// occurs--unless it has a stated duration".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Delayed {
    /// "At the beginning of your next main phase" (Mana Drain): the controller's next
    /// precombat or postcombat main phase.
    #[default]
    YourNextMainPhase,
}

/// A replacement effect of a permanent's own static ability that modifies how it enters
/// ([CR 614.1c](https://yawgatog.com/resources/magic-rules/#R6141c)): "Effects that read
/// '[This permanent] enters with . . . ,' 'As [this permanent] enters . . . ,' or '[This
/// permanent] enters as . . . ' are replacement effects."
///
/// [CR 614.12](https://yawgatog.com/resources/magic-rules/#R61412): "Some replacement effects
/// modify how a permanent enters the battlefield. ... Such effects may come from the
/// permanent itself if they affect only that permanent".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Replacement {
    None,
    /// "This enters tapped."
    EntersTapped,
    /// "This enters tapped unless you control N or fewer other lands." (Seachrome Coast)
    EntersTappedUnlessFewLands(u8),
    /// "As this enters, you may pay N life. If you don't, it enters tapped." (Soporific
    /// Springs) Whoever puts it onto the battlefield asks its controller first.
    EntersTappedUnlessPayLife(u8),
}

/// A static ability's continuous effect: what it affects, when, and how (§9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayerSpec {
    pub affected: Affected,
    pub condition: StaticCondition,
    pub modification: Modification,
}

/// Which permanent a static ability affects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Affected {
    /// The permanent with the ability.
    Itself,
    /// The permanent an Aura with the ability enchants.
    Enchanted,
}

/// When a static ability applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StaticCondition {
    Always,
    /// "As long as it has a [kind] counter on it" (impending's "isn't a creature").
    HasCounter(CounterKind),
}

/// What a continuous effect does to a permanent. The layer it applies in follows from the
/// modification ([CR 613.1](https://yawgatog.com/resources/magic-rules/#R6131)).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Modification {
    /// Layer 2: "gain control of".
    Control(u8),
    /// Layer 4: "becomes [types] in addition to its other types".
    AddTypes(Types),
    /// Layer 4: "isn't a [type]".
    RemoveTypes(Types),
    /// Layer 4: "is a [types] and loses all other card types". Subtypes go with the card
    /// types they belong to; subtypes it gains are added separately.
    SetTypes(Types),
    /// Layer 4: "is a [subtype]".
    AddSubtype(Subtype),
    /// Layer 6: "gains [keywords]".
    AddKeywords(Keywords),
    /// Layer 6: "has [ability]": the ability at this index on the front face of the effect
    /// source's card, such as the crew ability Swift Reconfiguration grants.
    Grant(u8),
    /// Layer 7a: a characteristic-defining ability, "power is equal to the number of
    /// creatures you control" (Adeline).
    PowerIsCreaturesYouControl,
    /// Layer 7c: "gets +X/+Y".
    ModifyPt(i8, i8),
}

/// The layers and sublayers continuous effects apply in, in order
/// ([CR 613.1](https://yawgatog.com/resources/magic-rules/#R6131),
/// [CR 613.4](https://yawgatog.com/resources/magic-rules/#R6134)). Layer 1, copy effects, is
/// applied by choosing the copiable values before any of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layer {
    Control = 2,
    Type = 4,
    Ability = 6,
    /// 7a: characteristic-defining abilities that define power and toughness.
    PtDefining = 7,
    /// 7c: effects and counters that modify power and toughness.
    PtModifying = 9,
}

impl Modification {
    pub fn layer(self) -> Layer {
        match self {
            Modification::Control(_) => Layer::Control,
            Modification::AddTypes(_)
            | Modification::RemoveTypes(_)
            | Modification::SetTypes(_)
            | Modification::AddSubtype(_) => Layer::Type,
            Modification::AddKeywords(_) | Modification::Grant(_) => Layer::Ability,
            Modification::PowerIsCreaturesYouControl => Layer::PtDefining,
            Modification::ModifyPt(..) => Layer::PtModifying,
        }
    }
}

/// The result of a body. `Err(Pause)` means the body asked a question its answer source could
/// not answer yet (§7.3). The engine keeps the question with the choice record rather than in
/// the `Err`, so `Flow` stays one byte wide.
pub type Flow = Result<(), Pause>;

/// A body stopped to wait for an answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pause;
