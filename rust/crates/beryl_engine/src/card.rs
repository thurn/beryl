//! Card definitions: typed data for everything the engine inspects, Rust functions for what an
//! ability does (architecture §6.1, §6.2, decision D1).
//!
//! Every type here is `'static` table data shared by all threads. `State` never holds one;
//! it stores game-table indices (§6.4) and ability indices instead.

use crate::effect::EffectCtx;
use crate::filter::Filter;

/// One card: its name and its faces.
#[derive(Debug)]
pub struct CardDef {
    pub name: &'static str,
    /// One face, or two for double-faced, modal double-faced, split and adventure cards.
    pub faces: &'static [Face],
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
    Keywords(u64) {}
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
}

impl Ability {
    /// A mana ability with the cost {T}, such as the intrinsic ability of a basic land type
    /// ([CR 305.6](https://yawgatog.com/resources/magic-rules/#R3056)): "An object with the
    /// land card type and a basic land type has the intrinsic ability '{T}: Add [mana
    /// symbol],' even if the text box doesn't actually contain that text."
    pub const fn tap_for_mana(produces: ManaSet) -> Ability {
        Ability {
            kind: AbilityKind::Mana,
            cost: Cost::TAP,
            timing: Timing::Instant,
            targets: &[],
            produces,
            may_pause: false,
            body: crate::effect::add_produced_mana,
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
