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

            /// Every member with the name of its constant, in the order written, so that a
            /// tool can name each member ([`crate::render`]) and a new member is named the
            /// moment it is added: `FIRST_STRIKE` is "first strike".
            pub const NAMED: &[($name, &str)] = &[$(($name::$flag, stringify!($flag))),*];

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
    /// pool's cards have: a test holds them to the catalogs and every pool card's type line
    /// (`beryl_card_build`'s `printed::TYPES`). The low byte holds every type a permanent can
    /// have, so a permanent's types fit a byte
    /// ([CR 110.4](https://yawgatog.com/resources/magic-rules/#R1104)): "There are six
    /// permanent types: artifact, battle, creature, enchantment, land, and planeswalker.
    /// Instant and sorcery cards can't enter the battlefield and thus can't be permanents.
    /// Some kindred cards can enter the battlefield and some can't".
    Types(u16) {
        ARTIFACT = 0,
        BATTLE = 1,
        CREATURE = 2,
        ENCHANTMENT = 3,
        KINDRED = 4,
        LAND = 5,
        PLANESWALKER = 6,
        INSTANT = 8,
        SORCERY = 9,
        CONSPIRACY = 10,
    }
}

bitset! {
    /// Supertypes ([CR 205.4a](https://yawgatog.com/resources/magic-rules/#R2054a)), those the
    /// pool's cards have, held to them as [`Types`] is.
    Supertypes(u8) {
        BASIC = 0,
        LEGENDARY = 1,
        SNOW = 2,
        WORLD = 3,
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
        /// [CR 702.19b](https://yawgatog.com/resources/magic-rules/#R70219b): "The
        /// controller of an attacking creature with trample first assigns damage to the
        /// creature(s) blocking it. Once all those blocking creatures are assigned lethal
        /// damage, any excess damage is assigned as its controller chooses among those
        /// blocking creatures and the player, planeswalker, or battle the creature is
        /// attacking."
        TRAMPLE = 11,
        /// [CR 702.15b](https://yawgatog.com/resources/magic-rules/#R70215b): "Damage dealt by
        /// a source with lifelink causes that source's controller, or its owner if it has no
        /// controller, to gain that much life (in addition to any other results that damage
        /// causes)."
        LIFELINK = 12,
        /// [CR 702.12b](https://yawgatog.com/resources/magic-rules/#R70212b): "A permanent
        /// with indestructible can't be destroyed. Such permanents aren't destroyed by lethal
        /// damage, and they ignore the state-based action that checks for lethal damage (see
        /// rule 704.5g)."
        INDESTRUCTIBLE = 13,
        /// Landwalk for each basic land type, a bit each
        /// ([CR 702.14c](https://yawgatog.com/resources/magic-rules/#R70214c)): "A creature
        /// with landwalk can't be blocked as long as the defending player controls at least
        /// one land with the specified land type (as in “islandwalk”)". [`Keywords::LANDWALK`]
        /// names the land type of each.
        PLAINSWALK = 16,
        ISLANDWALK = 17,
        SWAMPWALK = 18,
        MOUNTAINWALK = 19,
        FORESTWALK = 20,
    }
}

impl Keywords {
    /// Each landwalk keyword with the land subtype it names
    /// ([CR 702.14a](https://yawgatog.com/resources/magic-rules/#R70214a)): "Landwalk is a
    /// generic term that appears within an object's rules text as “[type]walk,” where [type]
    /// is usually a land type". Those for the basic land types are all the engine has.
    pub const LANDWALK: [(Keywords, Subtype); 5] = [
        (Keywords::PLAINSWALK, Subtype::Plains),
        (Keywords::ISLANDWALK, Subtype::Island),
        (Keywords::SWAMPWALK, Subtype::Swamp),
        (Keywords::MOUNTAINWALK, Subtype::Mountain),
        (Keywords::FORESTWALK, Subtype::Forest),
    ];

    /// Every landwalk keyword.
    pub const ANY_LANDWALK: Keywords = Keywords::PLAINSWALK
        .union(Keywords::ISLANDWALK)
        .union(Keywords::SWAMPWALK)
        .union(Keywords::MOUNTAINWALK)
        .union(Keywords::FORESTWALK);

    /// The land subtypes the landwalk keywords among `self` name.
    pub fn landwalk(self) -> Subtypes {
        let mut set = Subtypes::NONE;
        for (keyword, subtype) in Self::LANDWALK {
            if self.contains(keyword) {
                set = set.union(Subtypes::of(&[subtype]));
            }
        }
        set
    }
}

include!(concat!(env!("OUT_DIR"), "/subtypes.rs"));

/// A set of [`Subtype`]s, one bit each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Subtypes(pub [u64; SUBTYPE_WORDS]);

impl Subtypes {
    pub const NONE: Subtypes = Subtypes([0; SUBTYPE_WORDS]);

    pub const fn of(subtypes: &[Subtype]) -> Subtypes {
        let mut bits = [0; SUBTYPE_WORDS];
        let mut i = 0;
        while i < subtypes.len() {
            let s = subtypes[i] as usize;
            bits[s >> 6] |= 1 << (s & 63);
            i += 1;
        }
        Subtypes(bits)
    }

    /// Every subtype of a card type. `Subtypes::every(Types::CREATURE)` is what a changeling
    /// is ([CR 702.73a](https://yawgatog.com/resources/magic-rules/#R70273a)): "Changeling is
    /// a characteristic-defining ability. “Changeling” means “This object is every creature
    /// type.”"
    pub const fn every(card_type: Types) -> Subtypes {
        let mut bits = [0; SUBTYPE_WORDS];
        let mut i = 0;
        while i < Subtype::ALL.len() {
            if Subtype::ALL[i].card_type().0 & card_type.0 != 0 {
                bits[i >> 6] |= 1 << (i & 63);
            }
            i += 1;
        }
        Subtypes(bits)
    }

    /// The subtypes in either set.
    #[must_use]
    pub const fn union(self, other: Subtypes) -> Subtypes {
        let mut bits = self.0;
        let mut i = 0;
        while i < SUBTYPE_WORDS {
            bits[i] |= other.0[i];
            i += 1;
        }
        Subtypes(bits)
    }

    /// Whether every subtype of `other` is in the set.
    pub fn contains_all(self, other: Subtypes) -> bool {
        self.0.iter().zip(other.0).all(|(a, b)| a & b == b)
    }

    /// The subtypes in the set but not in `other`.
    #[must_use]
    pub fn minus(self, other: Subtypes) -> Subtypes {
        let mut bits = self.0;
        for (a, b) in bits.iter_mut().zip(other.0) {
            *a &= !b;
        }
        Subtypes(bits)
    }

    pub fn contains(self, subtype: Subtype) -> bool {
        let s = subtype as usize;
        self.0[s >> 6] & (1 << (s & 63)) != 0
    }

    #[must_use]
    pub fn with(self, subtype: Subtype) -> Subtypes {
        let mut bits = self.0;
        let s = subtype as usize;
        bits[s >> 6] |= 1 << (s & 63);
        Subtypes(bits)
    }

    /// The subtypes in the set, in [`Subtype`]'s order.
    pub fn iter(self) -> impl Iterator<Item = Subtype> {
        self.0.into_iter().enumerate().flat_map(|(word, mut bits)| {
            std::iter::from_fn(move || {
                if bits == 0 {
                    return None;
                }
                let bit = bits.trailing_zeros() as usize;
                bits &= bits - 1;
                Some(Subtype::ALL[word * 64 + bit])
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Subtype, Subtypes, Types};

    /// A set lists what was put in it, in order, across words.
    #[test]
    fn a_set_lists_its_members() {
        let last = Subtype::ALL[Subtype::ALL.len() - 1];
        let set = Subtypes::of(&[last, Subtype::Island, Subtype::Plains]);
        assert_eq!(
            set.iter().collect::<Vec<_>>(),
            vec![Subtype::Island, Subtype::Plains, last]
        );
        assert!(set.contains(last) && !set.contains(Subtype::Aura));
    }

    /// CR 205.3m: creatures and kindreds share the creature types, and a changeling has
    /// every one of them and nothing else (CR 702.73a).
    #[test]
    fn every_creature_type() {
        let every = Subtypes::every(Types::CREATURE);
        assert!(every.contains(Subtype::TimeLord) && every.contains(Subtype::Human));
        assert!(!every.contains(Subtype::Island) && !every.contains(Subtype::Jace));
        assert!(Subtype::Human.card_type().contains(Types::KINDRED));
        assert_eq!(
            every.iter().count(),
            Subtype::ALL
                .iter()
                .filter(|s| s.card_type().intersects(Types::CREATURE))
                .count()
        );
        assert_eq!(Subtype::TimeLord.name(), "Time Lord");
        assert_eq!(Subtype::Treasure.card_type(), Types::ARTIFACT);
        assert_eq!(
            Subtype::Lesson.card_type(),
            Types::INSTANT.union(Types::SORCERY)
        );
    }
}
