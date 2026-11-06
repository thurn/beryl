//! Card definitions: typed data for everything the engine inspects, Rust functions for what an
//! ability does (architecture §6.1, §6.2, decision D1).
//!
//! Every type here is `'static` table data shared by all threads. `State` never holds one;
//! it stores game-table indices (§6.4) and ability indices instead.

use crate::effect::EffectCtx;
use crate::filter::{Filter, Whose};
use crate::ids::Seat;
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
        self.face(FaceIx::FRONT)
    }

    /// The face at `face`, or for [`FaceIx::DOWN`] the characteristics every face-down
    /// permanent has, [`FACE_DOWN`].
    pub fn face(&self, face: FaceIx) -> &'static Face {
        if face == FaceIx::DOWN {
            return &FACE_DOWN;
        }
        &self.faces[usize::from(face.0)]
    }

    /// Each of its faces' indices, front first.
    pub fn face_ixs(&self) -> impl Iterator<Item = FaceIx> {
        (0..self.faces.len()).map(|i| FaceIx(u8::try_from(i).expect("a few faces")))
    }

    /// The name of the face at `face`, the name a spell or permanent with that face up has
    /// ([CR 712.8f](https://yawgatog.com/resources/magic-rules/#R7128f)): "While a modal
    /// double-faced spell is on the stack or a modal double-faced permanent is on the
    /// battlefield, it has only the characteristics of the face that's up." The same holds of
    /// a nonmodal one ([CR 712.8e](https://yawgatog.com/resources/magic-rules/#R7128e)):
    /// "While a nonmodal double-faced permanent has its back face up, it has only the
    /// characteristics of its back face." A face-down permanent has no name (CR 708.2a): the
    /// empty string.
    pub fn face_name(&self, face: FaceIx) -> &'static str {
        if face == FaceIx::FRONT {
            self.name
        } else {
            self.face(face).name
        }
    }
}

/// Which of a card's faces: an index into [`CardDef::faces`], read with [`CardDef::face`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FaceIx(u8);

impl FaceIx {
    /// The front face, a card's only face unless it has two.
    pub const FRONT: FaceIx = FaceIx(0);
    /// The back face of a double-faced card.
    pub const BACK: FaceIx = FaceIx(1);
    /// Face down: a permanent whose card is turned face down
    /// ([CR 110.5](https://yawgatog.com/resources/magic-rules/#R1105)): "A permanent's status
    /// is its physical state. There are four status categories, each of which has two possible
    /// values: tapped/untapped, flipped/unflipped, face up/face down, and phased in/phased
    /// out." Its characteristics are [`FACE_DOWN`]'s, whichever card it is, so every reader
    /// of [`CardDef::face`] sees what a face-down permanent has.
    pub const DOWN: FaceIx = FaceIx(u8::MAX);

    /// The index in [`CardDef::faces`], as wire formats write it.
    pub fn get(self) -> u8 {
        self.0
    }
}

/// A set of a face's casting options ([`Face::casting`]): those chosen for one cast, as its
/// decision option and stack entry record them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Castings(u8);

impl Castings {
    pub const NONE: Castings = Castings(0);

    /// The option at index `i` in [`Face::casting`] alone.
    pub const fn only(i: u8) -> Castings {
        assert!(i < 8, "eight options at most");
        Castings(1 << i)
    }

    /// Every set of `face`'s options, the empty one first.
    pub fn subsets(face: &Face) -> impl Iterator<Item = Castings> {
        let n = face.casting.len();
        debug_assert!(n <= 8);
        (0..(1u16 << n)).map(|m| Castings(u8::try_from(m).expect("eight options at most")))
    }

    /// Whether it has the option at index `i` in [`Face::casting`].
    pub fn contains(self, i: usize) -> bool {
        i < 8 && self.0 & (1 << i) != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The set as a bitmask over [`Face::casting`], the form scenario files are exported in.
    pub fn bits(self) -> u8 {
        self.0
    }

    /// The indices in [`Face::casting`] of its options, in order.
    pub fn indices(self) -> impl Iterator<Item = u8> {
        (0..8).filter(move |&i| self.0 & (1 << i) != 0)
    }

    /// The options of `face` it has, in the face's order.
    pub fn paid(self, face: &Face) -> impl Iterator<Item = CastingOption> + '_ {
        face.casting
            .iter()
            .enumerate()
            .filter(move |&(i, _)| self.contains(i))
            .map(|(_, &o)| o)
    }
}

/// The characteristics printed on one face.
#[derive(Debug)]
pub struct Face {
    /// The name of a back face. A front face's name is its card's, [`CardDef::name`], which
    /// is the card's name wherever it is not a permanent or spell with another face up
    /// ([CR 712.8a](https://yawgatog.com/resources/magic-rules/#R7128a)): "While a
    /// double-faced card is outside the game or in a zone other than the battlefield or
    /// stack, it has only the characteristics of its front face."
    pub name: &'static str,
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
    /// The alternative and additional costs it may be cast with, in the order its text lists
    /// them. A stack entry records the ones paid as a [`Castings`] set over this list.
    pub casting: &'static [CastingOption],
    /// The back face of a transforming double-faced card, which is never cast: "A
    /// double-faced spell is cast with its front face up by default"
    /// ([CR 712.11](https://yawgatog.com/resources/magic-rules/#R71211)). Only a modal
    /// double-faced card lets its caster choose the face
    /// ([CR 712.11b](https://yawgatog.com/resources/magic-rules/#R71211b)).
    pub transformed: bool,
    /// "As an additional cost to cast this spell, exile N cards from your graveyard"
    /// (Abhorrent Oculus): N, or 0 for no such cost. It is paid with the rest of the total
    /// cost ([CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h)).
    pub exile_from_graveyard: u8,
    /// Abilities this face grants to another object rather than has itself, such as the crew
    /// ability of Swift Reconfiguration's "Enchanted permanent is a Vehicle artifact with crew
    /// 5". [`Modification::Grant`] names one by its index here.
    pub grants: &'static [Ability],
}

impl Face {
    /// Whether this face is an instant or a sorcery: its spell resolves by following its
    /// instructions, not by becoming a permanent
    /// ([CR 608.2](https://yawgatog.com/resources/magic-rules/#R6082),
    /// [CR 608.3](https://yawgatog.com/resources/magic-rules/#R6083)).
    pub fn is_instant_or_sorcery(&self) -> bool {
        self.types.intersects(Types::INSTANT.union(Types::SORCERY))
    }

    /// The spell ability of an instant or sorcery, or the targeting ability of an Aura
    /// spell: what the spell targets and does as it resolves.
    pub fn spell_ability(&self) -> Option<&'static Ability> {
        self.abilities.iter().find(|a| a.kind == AbilityKind::Spell)
    }

    /// The spell ability of this face cast with the options `costs`: its overloaded one when
    /// its overload cost is paid ([`CastingOption::Overload`]), else
    /// [`Face::spell_ability`].
    pub fn spell_ability_with(&self, costs: Castings) -> Option<&'static Ability> {
        costs
            .paid(self)
            .find_map(|o| match o {
                CastingOption::Overload { ability, .. } => {
                    Some(&self.abilities[usize::from(ability)])
                }
                _ => None,
            })
            .or_else(|| self.spell_ability())
    }

    /// The colors of the card that paying `costs` exiles from its caster's hand, such as
    /// Subtlety's evoke cost, "Exile a blue card from your hand"; none for no such card.
    pub fn exiles_with(&self, costs: Castings) -> Colors {
        costs.paid(self).fold(Colors::NONE, |colors, o| match o {
            CastingOption::Evoke { exile, .. } => colors.union(exile),
            _ => colors,
        })
    }
}

/// What a face-down permanent is
/// ([CR 708.2a](https://yawgatog.com/resources/magic-rules/#R7082a)): "A permanent that enters
/// the battlefield face down also has these characteristics unless otherwise specified by the
/// effect that put it onto the battlefield face down": "a 2/2 face-down creature with no text,
/// no name, no subtypes, and no mana cost." These are its copiable values, applied in layer 1b
/// ([CR 613.2b](https://yawgatog.com/resources/magic-rules/#R6132b)): "Face-down spells and
/// permanents have their characteristics modified as defined in rule 708.2." The cube's only
/// face-down permanents are manifested ones (CR 701.40a), which list the same values.
pub static FACE_DOWN: Face = Face {
    types: Types::CREATURE,
    power: Some(2),
    toughness: Some(2),
    ..Face::EMPTY
};

impl Face {
    /// A face with no cost, types or abilities, for `..Face::EMPTY` in card files.
    pub const EMPTY: Face = Face {
        name: "",
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
        casting: &[],
        transformed: false,
        exile_from_graveyard: 0,
        grants: &[],
    };

    /// One of this face's own abilities or of those it grants.
    pub fn ability(&'static self, ability: FaceAbility) -> &'static Ability {
        match ability {
            FaceAbility::Own(i) => &self.abilities[usize::from(i)],
            FaceAbility::Grant(i) => &self.grants[usize::from(i)],
        }
    }
}

/// An ability printed on a face, as a stack entry names it: one the face has itself, by its
/// index in [`Face::abilities`], or one the face grants to another object, by its index in
/// [`Face::grants`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FaceAbility {
    Own(u8),
    Grant(u8),
}

/// An ability of a permanent, as [`crate::decision::Opt::Activate`] names it: one of its face
/// up's own, by its index in [`Face::abilities`], or the `k`th ability other objects grant
/// it, by its index in [`crate::state::Derived::granted`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AbilityIx {
    Own(u8),
    Granted(u8),
}

/// A mana cost: generic mana plus colored and colorless symbols, and any {X}.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ManaCost {
    pub generic: u8,
    /// Symbols by [`ManaColor`] index: W, U, B, R, G, C.
    pub symbols: [u8; 6],
    /// How many {X} symbols it has
    /// ([CR 107.3a](https://yawgatog.com/resources/magic-rules/#R1073a)): their value is
    /// announced as the spell is cast or the ability activated.
    pub x: u8,
}

impl ManaCost {
    pub const NONE: ManaCost = ManaCost {
        generic: 0,
        symbols: [0; 6],
        x: 0,
    };

    /// A cost of generic mana and colored or colorless symbols, by [`ManaColor`] index.
    pub const fn new(generic: u8, symbols: [u8; 6]) -> ManaCost {
        ManaCost {
            generic,
            symbols,
            x: 0,
        }
    }

    /// A cost of generic mana only, such as {3}.
    pub const fn generic(n: u8) -> ManaCost {
        ManaCost::new(n, [0; 6])
    }

    /// A cost of {X} alone, such as Thieving Skydiver's kicker.
    pub const X: ManaCost = ManaCost {
        generic: 0,
        symbols: [0; 6],
        x: 1,
    };

    /// The amount of mana paying it takes once X is `x`.
    pub fn total(self, x: u8) -> u32 {
        u32::from(self.generic)
            + self.symbols.iter().map(|&n| u32::from(n)).sum::<u32>()
            + u32::from(self.x) * u32::from(x)
    }

    /// Both costs together, X included: what
    /// ([CR 601.2f](https://yawgatog.com/resources/magic-rules/#R6012f)) totals as "the mana
    /// cost or alternative cost ..., plus all additional costs and cost increases".
    #[must_use]
    pub fn plus(self, other: ManaCost) -> ManaCost {
        let mut sum = self;
        sum.generic = sum.generic.saturating_add(other.generic);
        for (a, b) in sum.symbols.iter_mut().zip(other.symbols) {
            *a = a.saturating_add(b);
        }
        sum.x = sum.x.saturating_add(other.x);
        sum
    }

    /// This cost with X replaced by its announced value, as generic mana
    /// ([CR 107.4b](https://yawgatog.com/resources/magic-rules/#R1074b)): "Numerical symbols
    /// (such as {1}) and variable symbols (such as {X}) represent generic mana in costs."
    #[must_use]
    pub fn with_x(self, x: u8) -> ManaCost {
        ManaCost {
            generic: self.generic.saturating_add(self.x.saturating_mul(x)),
            x: 0,
            ..self
        }
    }

    pub fn is_zero(self) -> bool {
        self.total(0) == 0 && self.x == 0
    }

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

impl ManaColor {
    /// Every kind, in the order W, U, B, R, G, C that indexes [`ManaCost::symbols`].
    pub const ALL: [ManaColor; 6] = [
        ManaColor::White,
        ManaColor::Blue,
        ManaColor::Black,
        ManaColor::Red,
        ManaColor::Green,
        ManaColor::Colorless,
    ];

    /// Its index in [`ManaColor::ALL`].
    pub fn index(self) -> usize {
        self as usize
    }
}

// `ManaColor::index` is the discriminant, so `ManaColor::ALL` lists the kinds in declaration
// order.
const _: () = {
    let mut i = 0;
    while i < ManaColor::ALL.len() {
        assert!(ManaColor::ALL[i] as usize == i);
        i += 1;
    }
};

/// What a mana ability can produce: a set of [`ManaColor`]s and how much.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
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

            /// The members of `self` not in `other`.
            #[must_use]
            pub const fn minus(self, other: $name) -> $name {
                $name(self.0 & !other.0)
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
        /// [CR 702.8a](https://yawgatog.com/resources/magic-rules/#R7028a): "'Flash' means
        /// 'You may play this card any time you could cast an instant.'"
        FLASH = 1,
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
    /// An artifact type (CR 301.7), of the Map tokens Get Lost creates.
    Map,
    /// An artifact type (CR 301.7), of Treasure tokens
    /// ([CR 111.10a](https://yawgatog.com/resources/magic-rules/#R11110a)).
    Treasure,
    // Creature types ([CR 205.3m](https://yawgatog.com/resources/magic-rules/#R2053m)).
    Avatar,
    Cleric,
    Elemental,
    Elk,
    Eye,
    Fish,
    Horror,
    Human,
    Illusion,
    Incarnation,
    Insect,
    Knight,
    Merfolk,
    Otter,
    Pirate,
    Rogue,
    Samurai,
    Siren,
    Sphinx,
    Spirit,
    Wizard,
    // Planeswalker types ([CR 205.3j](https://yawgatog.com/resources/magic-rules/#R2053j)).
    Jace,
    Teferi,
}

impl Subtype {
    pub const ALL: [Subtype; 32] = [
        Subtype::Plains,
        Subtype::Island,
        Subtype::Swamp,
        Subtype::Mountain,
        Subtype::Forest,
        Subtype::Vehicle,
        Subtype::Aura,
        Subtype::Map,
        Subtype::Treasure,
        Subtype::Avatar,
        Subtype::Cleric,
        Subtype::Elemental,
        Subtype::Elk,
        Subtype::Eye,
        Subtype::Fish,
        Subtype::Horror,
        Subtype::Human,
        Subtype::Illusion,
        Subtype::Incarnation,
        Subtype::Insect,
        Subtype::Knight,
        Subtype::Merfolk,
        Subtype::Otter,
        Subtype::Pirate,
        Subtype::Rogue,
        Subtype::Samurai,
        Subtype::Siren,
        Subtype::Sphinx,
        Subtype::Spirit,
        Subtype::Wizard,
        Subtype::Jace,
        Subtype::Teferi,
    ];

    /// The card type this subtype belongs to (CR 205.3).
    pub const fn card_type(self) -> Types {
        match self {
            Subtype::Plains
            | Subtype::Island
            | Subtype::Swamp
            | Subtype::Mountain
            | Subtype::Forest => Types::LAND,
            Subtype::Vehicle | Subtype::Map | Subtype::Treasure => Types::ARTIFACT,
            Subtype::Aura => Types::ENCHANTMENT,
            Subtype::Avatar
            | Subtype::Cleric
            | Subtype::Elemental
            | Subtype::Eye
            | Subtype::Elk
            | Subtype::Fish
            | Subtype::Horror
            | Subtype::Human
            | Subtype::Illusion
            | Subtype::Incarnation
            | Subtype::Insect
            | Subtype::Knight
            | Subtype::Merfolk
            | Subtype::Otter
            | Subtype::Pirate
            | Subtype::Rogue
            | Subtype::Samurai
            | Subtype::Siren
            | Subtype::Sphinx
            | Subtype::Spirit
            | Subtype::Wizard => Types::CREATURE,
            Subtype::Jace | Subtype::Teferi => Types::PLANESWALKER,
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

/// An activation cost: mana, and the other things paying it takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Cost {
    pub mana: ManaCost,
    /// The tap symbol ([CR 107.5](https://yawgatog.com/resources/magic-rules/#R1075)): "The tap
    /// symbol in an activation cost means 'Tap this permanent.'"
    pub tap: bool,
    /// "Sacrifice this permanent"
    /// ([CR 701.21a](https://yawgatog.com/resources/magic-rules/#R70121a)): "To sacrifice a
    /// permanent, its controller moves it from the battlefield directly to its owner's
    /// graveyard."
    pub sacrifice_self: bool,
    /// "Discard this card". Only a card in a hand can be discarded, so an ability with this
    /// cost is activated from its owner's hand, as cycling is.
    pub discard_self: bool,
    /// A loyalty ability's cost: loyalty counters to put on (positive) or remove (negative)
    /// ([CR 606.4](https://yawgatog.com/resources/magic-rules/#R6064)). `Some(0)` is a
    /// loyalty ability too.
    pub loyalty: Option<i8>,
    /// Crew's cost: tap other untapped creatures you control with at least this much total
    /// power, or 0 for none
    /// ([CR 702.122a](https://yawgatog.com/resources/magic-rules/#R702122a)): "'Crew N' means
    /// 'Tap any number of other untapped creatures you control with total power N or greater:
    /// This permanent becomes an artifact creature until end of turn.'"
    pub crew: u8,
    /// "Remove a [kind] counter from this permanent", as Staff of the Storyteller's.
    pub remove: Option<CounterKind>,
    /// How much of the generic mana is a waterbend cost's
    /// ([CR 701.67a](https://yawgatog.com/resources/magic-rules/#R70167a)): "'Waterbend
    /// [cost]' means 'Pay [cost]. For each generic mana in that cost, you may tap an untapped
    /// artifact or creature you control rather than pay that mana.'" Only that much may be
    /// paid by tapping ([CR 701.67b](https://yawgatog.com/resources/magic-rules/#R70167b)).
    pub waterbend: u8,
}

impl Cost {
    pub const NONE: Cost = Cost {
        mana: ManaCost::NONE,
        tap: false,
        sacrifice_self: false,
        discard_self: false,
        loyalty: None,
        crew: 0,
        remove: None,
        waterbend: 0,
    };
    pub const TAP: Cost = Cost {
        tap: true,
        ..Cost::NONE
    };

    /// A cost of mana only.
    pub const fn mana(mana: ManaCost) -> Cost {
        Cost { mana, ..Cost::NONE }
    }

    /// A loyalty ability's cost.
    pub const fn loyalty(n: i8) -> Cost {
        Cost {
            loyalty: Some(n),
            ..Cost::NONE
        }
    }
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

/// What kind of thing a target is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TargetKind {
    /// A permanent: an object on the battlefield.
    Permanent,
    /// A spell on the stack.
    Spell,
    /// A spell on the stack or a permanent, such as "target spell or nonland permanent".
    SpellOrPermanent,
    /// A card in a graveyard, such as "target instant or sorcery card from your graveyard".
    GraveyardCard,
    /// A player. The filter may be [`Filter::Any`] or [`Filter::Whose`], read for players.
    Player,
}

/// One instance of the word "target" in a spell or ability: what it may choose and how many
/// ([CR 115.1](https://yawgatog.com/resources/magic-rules/#R1151)).
#[derive(Clone, Copy, Debug)]
pub struct TargetSpec {
    pub kind: TargetKind,
    pub filter: Filter,
    /// Exactly this many, or up to this many when `optional`
    /// ([CR 115.6](https://yawgatog.com/resources/magic-rules/#R1156)): "A spell or ability
    /// that requires targets may allow zero targets to be chosen."
    pub count: u8,
    pub optional: bool,
    /// "Other target": not the object whose ability this is.
    pub other: bool,
}

impl TargetSpec {
    /// One target of a kind that passes a filter.
    pub const fn one(kind: TargetKind, filter: Filter) -> TargetSpec {
        TargetSpec {
            kind,
            filter,
            count: 1,
            optional: false,
            other: false,
        }
    }

    /// "Up to one target".
    pub const fn up_to_one(kind: TargetKind, filter: Filter) -> TargetSpec {
        TargetSpec {
            optional: true,
            ..TargetSpec::one(kind, filter)
        }
    }

    /// The same spec for "another target": its ability's source can't be chosen.
    #[must_use]
    pub const fn other(self) -> TargetSpec {
        TargetSpec {
            other: true,
            ..self
        }
    }
}

/// An alternative or additional cost printed on a face, which its caster may choose to pay as
/// they cast it ([CR 601.2b](https://yawgatog.com/resources/magic-rules/#R6012b)).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CastingOption {
    /// [CR 702.33a](https://yawgatog.com/resources/magic-rules/#R70233a): "'Kicker [cost]'
    /// means 'You may pay an additional [cost] as you cast this spell.'" `min_x` is the
    /// least X a kicker of {X} allows, 1 for "X can't be 0".
    Kicker { cost: ManaCost, min_x: u8 },
    /// [CR 702.175a](https://yawgatog.com/resources/magic-rules/#R702175a): "'Offspring
    /// [cost]' means 'You may pay an additional [cost] as you cast this spell'".
    Offspring(ManaCost),
    /// [CR 702.176a](https://yawgatog.com/resources/magic-rules/#R702176a): "'Impending
    /// N--[cost]' means 'You may choose to pay [cost] rather than pay this spell's mana
    /// cost,' 'If you chose to pay this permanent's impending cost, it enters with N time
    /// counters on it'".
    Impending { cost: ManaCost, counters: u8 },
    /// [CR 702.96a](https://yawgatog.com/resources/magic-rules/#R70296a): "Overload [cost]"
    /// means "You may choose to pay [cost] rather than pay this spell's mana cost" and "If you
    /// chose to pay this spell's overload cost, change its text by replacing all instances of
    /// the word 'target' with the word 'each.'" The changed text is the face's spell ability
    /// at index `ability`, an untargeted [`AbilityKind::Spell`] the face lists after its own.
    Overload { cost: ManaCost, ability: u8 },
    /// [CR 702.74a](https://yawgatog.com/resources/magic-rules/#R70274a): "Evoke [cost]" means
    /// "You may cast this card by paying [cost] rather than paying its mana cost" and "When
    /// this permanent enters, if its evoke cost was paid, its controller sacrifices it." The
    /// cost is `cost` and, unless `exile` is empty, a card of one of those colors exiled from
    /// its caster's hand, as Subtlety's "Evoke--Exile a blue card from your hand." The face
    /// lists [`Ability::evoke`] too.
    Evoke { cost: ManaCost, exile: Colors },
}

impl CastingOption {
    /// Whether it is paid rather than the mana cost
    /// ([CR 118.9](https://yawgatog.com/resources/magic-rules/#R1189)) instead of on top of
    /// it.
    pub fn is_alternative(self) -> bool {
        matches!(
            self,
            CastingOption::Impending { .. }
                | CastingOption::Overload { .. }
                | CastingOption::Evoke { .. }
        )
    }

    /// Its mana cost: all of it, or the mana part of an evoke cost.
    pub fn cost(self) -> ManaCost {
        match self {
            CastingOption::Kicker { cost, .. }
            | CastingOption::Offspring(cost)
            | CastingOption::Impending { cost, .. }
            | CastingOption::Overload { cost, .. }
            | CastingOption::Evoke { cost, .. } => cost,
        }
    }
}

/// A static ability that changes a rule of the game rather than an object's characteristics
/// ([CR 604.1](https://yawgatog.com/resources/magic-rules/#R6041)): "Static abilities do
/// something all the time rather than being activated or triggered."
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Rule {
    None,
    /// "Each opponent can cast spells only any time they could cast a sorcery." (Teferi, Time
    /// Raveler)
    OpponentsCastAsSorceries,
    /// "As long as this entered this turn, you may activate its loyalty abilities any time you
    /// could cast an instant." (The Wandering Emperor)
    InstantLoyaltyTheTurnItEntered,
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
    /// Static abilities: the rule they change.
    pub rule: Rule,
    /// Mana abilities: "Activate only if you control a [permanent this filter accepts]"
    /// ([CR 602.5](https://yawgatog.com/resources/magic-rules/#R6025)): "A player can't begin
    /// to activate an ability that's prohibited from being activated."
    pub only_if_you_control: Option<Filter>,
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
        rule: Rule::None,
        only_if_you_control: None,
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

    /// Cycling ([CR 702.29a](https://yawgatog.com/resources/magic-rules/#R70229a)): "Cycling
    /// is an activated ability that functions only while the card with cycling is in a
    /// player's hand. 'Cycling [cost]' means '[Cost], Discard this card: Draw a card.'"
    pub const fn cycling(mana: ManaCost) -> Ability {
        Ability {
            kind: AbilityKind::Activated,
            cost: Cost {
                mana,
                discard_self: true,
                ..Cost::NONE
            },
            body: crate::effect::draw_a_card,
            ..Ability::NONE
        }
    }

    /// Crew N (CR 702.122a), an activated ability any time its controller has priority.
    pub const fn crew(n: u8) -> Ability {
        Ability {
            kind: AbilityKind::Activated,
            cost: Cost {
                crew: n,
                ..Cost::NONE
            },
            body: crate::effect::becomes_artifact_creature,
            ..Ability::NONE
        }
    }

    /// Offspring's triggered ability
    /// ([CR 702.175a](https://yawgatog.com/resources/magic-rules/#R702175a)): "When this
    /// permanent enters, if its offspring cost was paid, create a token that's a copy of it,
    /// except it's 1/1." The face lists [`CastingOption::Offspring`] too.
    pub const fn offspring() -> Ability {
        Ability {
            condition: Some(crate::effect::offspring_paid),
            ..Ability::triggered(Trigger::Enters, crate::effect::offspring_copy)
        }
    }

    /// Impending's static ability (CR 702.176a): "As long as this permanent's impending cost
    /// was paid and it has a time counter on it, it's not a creature." The face lists
    /// [`CastingOption::Impending`], whose counters the permanent enters with, and
    /// [`Ability::impending_countdown`].
    pub const fn impending() -> Ability {
        Ability::statik(LayerSpec {
            affected: Affected::Itself,
            condition: StaticCondition::Impending,
            modification: Modification::RemoveTypes(Types::CREATURE),
        })
    }

    /// Impending's triggered ability (CR 702.176a): "At the beginning of your end step, if
    /// this permanent's impending cost was paid and it has a time counter on it, remove a
    /// time counter from it."
    pub const fn impending_countdown() -> Ability {
        Ability {
            condition: Some(crate::effect::impending_counting_down),
            ..Ability::triggered(
                Trigger::Step(TurnStep::End, Whose::You),
                crate::effect::remove_a_time_counter,
            )
        }
    }

    /// Evoke's triggered ability
    /// ([CR 702.74a](https://yawgatog.com/resources/magic-rules/#R70274a)): "When this
    /// permanent enters, if its evoke cost was paid, its controller sacrifices it." The face
    /// lists [`CastingOption::Evoke`] too.
    pub const fn evoke() -> Ability {
        Ability {
            condition: Some(crate::effect::evoked),
            ..Ability::triggered(Trigger::Enters, crate::effect::sacrifice_itself)
        }
    }

    /// Whether this is a loyalty ability
    /// ([CR 606.1](https://yawgatog.com/resources/magic-rules/#R6061)).
    pub fn is_loyalty(&self) -> bool {
        self.cost.loyalty.is_some()
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
    /// "Whenever you discard a card": once for each card discarded
    /// ([CR 603.2c](https://yawgatog.com/resources/magic-rules/#R6032c)): "An ability triggers
    /// only once each time its trigger event occurs. However, it can trigger repeatedly if one
    /// event contains multiple occurrences." The discarding player is the card's owner
    /// ([CR 701.9a](https://yawgatog.com/resources/magic-rules/#R7019a)): "To discard a card,
    /// move it from its owner's hand to that player's graveyard."
    YouDiscard,
    /// "Whenever you draw a card": once for each card drawn (CR 603.2c), each draw one event
    /// of its own ([CR 121.2](https://yawgatog.com/resources/magic-rules/#R1212)): "If a
    /// player is instructed to draw multiple cards, that player performs that many individual
    /// card draws." A draw that a replacement effect replaced never happens
    /// ([CR 614.6](https://yawgatog.com/resources/magic-rules/#R6146)), so it triggers nothing.
    YouDraw,
    /// A delayed triggered ability (CR 603.7): never matched against events, but created by
    /// a body, to wait for the moment its text names.
    Delayed(Delayed),
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
    /// "At the beginning of the next end step" (Teferi, Hero of Dominaria): whoever's turn it
    /// is ([CR 513.1a](https://yawgatog.com/resources/magic-rules/#R5131a)).
    NextEndStep,
}

/// A replacement effect of a permanent's own static ability: most modify how it enters
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
    /// "If you would draw a card while your library has no cards in it, you win the game
    /// instead." (Jace, Wielder of Mysteries) Not a way of entering: a draw replacement
    /// ([CR 121.6a](https://yawgatog.com/resources/magic-rules/#R1216a)): "An effect that
    /// replaces a card draw is applied even if no cards could be drawn because there are no
    /// cards in the affected player's library."
    WinInsteadOfDrawingFromEmptyLibrary,
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
    /// "As long as it has a [kind] counter on it".
    HasCounter(CounterKind),
    /// Impending's "As long as this permanent's impending cost was paid and it has a time
    /// counter on it" (CR 702.176a).
    Impending,
}

/// What a continuous effect does to a permanent. The layer it applies in follows from the
/// modification ([CR 613.1](https://yawgatog.com/resources/magic-rules/#R6131)).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Modification {
    /// Layer 2: "gain control of".
    Control(Seat),
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
    /// Layer 6: "has [ability]": the ability at this index of the effect source's front
    /// face's [`Face::grants`], such as the crew ability Swift Reconfiguration grants.
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
