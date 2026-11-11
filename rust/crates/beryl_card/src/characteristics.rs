//! The characteristics a face has besides its cost and abilities: card types, supertypes,
//! subtypes, colors and keyword abilities, each a bit set.

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
