//! The subtypes of a game, numbered by its table, and the set of them a permanent has.
//!
//! [CR 205.3](https://yawgatog.com/resources/magic-rules/#R2053): the catalogs list over 500
//! subtypes (`beryl_card::Subtype`), too many for a bit each in every permanent's layer
//! results. A game can only ever meet the few its cards print, ask about or give, so the
//! [`crate::GameTable`] numbers those ([`SubtypeNumbers`]), as it numbers the game's cards,
//! and the state keeps a 128-bit [`GameSubtypes`] by those numbers. A game whose cards name
//! more than [`GAME_SUBTYPES`] fails to make a table
//! ([`crate::table::TableError::Subtypes`]).
//!
//! One bit stands for every creature type: what a changeling has
//! ([CR 702.73a](https://yawgatog.com/resources/magic-rules/#R70273a)): "Changeling is a
//! characteristic-defining ability. “Changeling” means “This object is every creature
//! type.”" A set with it contains every creature type, numbered by the game or not, and a
//! face that prints every creature type gets it in place of a number for each.
//!
//! An effect that lets a player choose a creature type
//! ([CR 205.3e](https://yawgatog.com/resources/magic-rules/#R2053e): "If an effect instructs
//! a player to choose a subtype, that player must choose one, and only one, existing
//! subtype") can choose one no card of the game names. None of the pool's cards with a file
//! has one. Such a card keeps the chosen `Subtype` in its own store and the table numbers a
//! subtype for each such choice its cards can make, which the choice is bound to as it is
//! made: the set stays 128 bits.

use beryl_card::{CardDef, Subtype, Subtypes, Types};

/// The subtypes a game numbers at most: a bit of [`GameSubtypes`] each, and one more for
/// every creature type.
pub const GAME_SUBTYPES: usize = 127;

/// The bit of every creature type.
const EVERY_CREATURE_TYPE: usize = 127;

/// No number: a subtype the game's cards never name.
const UNNUMBERED: u8 = u8::MAX;

/// The card types whose subtypes are the creature types (CR 205.3m).
const CREATURE_TYPES: Types = Types::CREATURE.union(Types::KINDRED);

/// Every creature type, found as the crate compiles.
const EVERY: Subtypes = Subtypes::every(CREATURE_TYPES);

/// The numbers a game gives the subtypes its cards name.
#[derive(Clone, Debug)]
pub struct SubtypeNumbers {
    /// Per [`Subtype`], its number, or [`UNNUMBERED`].
    numbers: Vec<u8>,
    /// Per [`Subtype`], the test of whether a set has it, found once per game: the filters
    /// that ask for a subtype run in the scans over every permanent.
    tests: Vec<HasSubtype>,
    /// Per number, its subtype.
    subtypes: Vec<Subtype>,
}

impl SubtypeNumbers {
    /// Numbers every subtype `defs` print, ask about or give (`CardDef::subtypes`), in
    /// [`Subtype`]'s order, so the same cards always number them alike. A face printing
    /// every creature type takes the bit of every creature type for them.
    ///
    /// # Errors
    ///
    /// How many there are, when that is more than [`GAME_SUBTYPES`].
    pub fn new(defs: &[&CardDef]) -> Result<Self, usize> {
        let mut named = Subtypes::NONE;
        for def in defs {
            def.subtypes(&mut |subtypes| {
                let subtypes = if subtypes.contains_all(EVERY) {
                    subtypes.minus(EVERY)
                } else {
                    subtypes
                };
                named = named.union(subtypes);
            });
        }
        let subtypes: Vec<Subtype> = named.iter().collect();
        if subtypes.len() > GAME_SUBTYPES {
            return Err(subtypes.len());
        }
        let mut numbers = vec![UNNUMBERED; Subtype::ALL.len()];
        for (n, &subtype) in subtypes.iter().enumerate() {
            numbers[subtype as usize] = u8::try_from(n).expect("at most 127 numbers");
        }
        let tests = Subtype::ALL
            .iter()
            .map(|&subtype| {
                let mask = GameSubtypes::NONE;
                let mask = match numbers[subtype as usize] {
                    UNNUMBERED => mask,
                    n => mask.with_bit(usize::from(n)),
                };
                HasSubtype(if subtype.card_type().intersects(CREATURE_TYPES) {
                    mask.with_bit(EVERY_CREATURE_TYPE)
                } else {
                    mask
                })
            })
            .collect();
        Ok(Self {
            numbers,
            tests,
            subtypes,
        })
    }

    /// The number of `subtype` in the game, if its cards name it.
    #[inline]
    fn number(&self, subtype: Subtype) -> Option<usize> {
        let n = self.numbers[subtype as usize];
        (n != UNNUMBERED).then_some(usize::from(n))
    }

    /// The printed subtypes of the first two faces of each of `defs`, by these numbers.
    pub fn of_cards(&self, defs: &[&CardDef]) -> Vec<[GameSubtypes; 2]> {
        defs.iter()
            .map(|d| {
                let of = |i: usize| {
                    d.faces
                        .get(i)
                        .map_or(GameSubtypes::NONE, |f| self.of(f.subtypes))
                };
                [of(0), of(1)]
            })
            .collect()
    }

    /// The test of whether a set has `subtype`: one AND, to ask of many sets.
    #[inline]
    pub fn has(&self, subtype: Subtype) -> HasSubtype {
        self.tests[subtype as usize]
    }

    /// A face's printed subtypes by the game's numbers. Every one is numbered, but those of
    /// a face printing every creature type, which take the one bit.
    pub fn of(&self, subtypes: Subtypes) -> GameSubtypes {
        let mut set = GameSubtypes::NONE;
        let subtypes = if subtypes.contains_all(EVERY) {
            set = set.with_bit(EVERY_CREATURE_TYPE);
            subtypes.minus(EVERY)
        } else {
            subtypes
        };
        for subtype in subtypes.iter() {
            if let Some(n) = self.number(subtype) {
                set = set.with_bit(n);
            }
        }
        set
    }
}

/// A set of subtypes by a game's numbers ([`SubtypeNumbers`]), with a bit for every creature
/// type (CR 702.73a). Its members are read through the game's table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct GameSubtypes([u64; 2]);

impl GameSubtypes {
    pub const NONE: GameSubtypes = GameSubtypes([0; 2]);

    #[inline]
    fn has_bit(self, bit: usize) -> bool {
        self.0[bit >> 6] & (1 << (bit & 63)) != 0
    }

    #[inline]
    fn with_bit(mut self, bit: usize) -> GameSubtypes {
        self.0[bit >> 6] |= 1 << (bit & 63);
        self
    }

    /// Whether the set has `subtype`, numbered by `numbers`. A subtype the game's cards never
    /// name is in no set, but a creature type in one with every creature type.
    #[inline]
    pub fn contains(self, numbers: &SubtypeNumbers, subtype: Subtype) -> bool {
        numbers.has(subtype).of(self)
    }

    /// The set with `subtype` added.
    ///
    /// # Panics
    ///
    /// In a debug build, when the game's cards never name `subtype`: `CardDef::subtypes`
    /// finds every subtype a definition's data gives, so only a Rust body could.
    #[inline]
    #[must_use]
    pub fn with(self, numbers: &SubtypeNumbers, subtype: Subtype) -> GameSubtypes {
        let Some(n) = numbers.number(subtype) else {
            debug_assert!(false, "no card of the game names the subtype {subtype:?}");
            return self;
        };
        self.with_bit(n)
    }

    /// The members correlated with one of `types`, the card types they belong to
    /// ([CR 205.3c](https://yawgatog.com/resources/magic-rules/#R2053c)).
    #[must_use]
    pub fn of_types(self, numbers: &SubtypeNumbers, types: Types) -> GameSubtypes {
        let mut kept = GameSubtypes::NONE;
        if self.has_bit(EVERY_CREATURE_TYPE) && types.intersects(CREATURE_TYPES) {
            kept = kept.with_bit(EVERY_CREATURE_TYPE);
        }
        for (n, subtype) in numbers.subtypes.iter().enumerate() {
            if self.has_bit(n) && types.intersects(subtype.card_type()) {
                kept = kept.with_bit(n);
            }
        }
        kept
    }

    /// The members correlated with none of `types`
    /// ([CR 205.3c](https://yawgatog.com/resources/magic-rules/#R2053c)): what is left when
    /// an effect sets a subtype of those card types.
    #[must_use]
    pub fn without_types(self, numbers: &SubtypeNumbers, types: Types) -> GameSubtypes {
        let of = self.of_types(numbers, types);
        GameSubtypes([self.0[0] & !of.0[0], self.0[1] & !of.0[1]])
    }

    /// The numbered members, in [`Subtype`]'s order; with every creature type, those the
    /// game numbers.
    pub fn iter(self, numbers: &SubtypeNumbers) -> impl Iterator<Item = Subtype> + '_ {
        numbers
            .subtypes
            .iter()
            .copied()
            .filter(move |&subtype| self.contains(numbers, subtype))
    }
}

/// Whether a set has one subtype ([`SubtypeNumbers::has`]): the subtype's bit, and the bit of
/// every creature type for a creature type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HasSubtype(GameSubtypes);

impl HasSubtype {
    /// Whether any set of the game can have the subtype: whether its cards name it, or it is
    /// a creature type.
    #[inline]
    pub fn possible(self) -> bool {
        self.0 != GameSubtypes::NONE
    }

    /// Whether `set` has the subtype.
    #[inline]
    pub fn of(self, set: GameSubtypes) -> bool {
        (set.0[0] & self.0.0[0]) | (set.0[1] & self.0.0[1]) != 0
    }
}

#[cfg(test)]
mod tests {
    use beryl_card::{CardDef, Face, Subtype, Subtypes, Types, card};

    use super::{GAME_SUBTYPES, GameSubtypes, SubtypeNumbers};

    static KNIGHT: CardDef = card(
        "Knight",
        &[Face::EMPTY
            .types(Types::CREATURE)
            .subtypes(Subtypes::of(&[Subtype::Human, Subtype::Knight]))],
    );
    static SHAPESHIFTER: CardDef = card(
        "Shapeshifter",
        &[Face::EMPTY
            .types(Types::CREATURE)
            .subtypes(Subtypes::every(Types::CREATURE))],
    );
    static LAND: CardDef = card(
        "Land",
        &[Face::EMPTY
            .types(Types::LAND)
            .subtypes(Subtypes::of(&[Subtype::Island]))],
    );

    /// A game numbers what its cards print, in order, and no other subtype is in a set.
    #[test]
    fn a_game_numbers_its_subtypes() {
        let numbers = SubtypeNumbers::new(&[&LAND, &KNIGHT]).unwrap();
        let knight = numbers.of(KNIGHT.faces[0].subtypes);
        assert!(knight.contains(&numbers, Subtype::Knight));
        assert!(!knight.contains(&numbers, Subtype::Island));
        assert!(!knight.contains(&numbers, Subtype::Elf));
        assert_eq!(
            knight.iter(&numbers).collect::<Vec<_>>(),
            vec![Subtype::Human, Subtype::Knight]
        );
        let land = numbers
            .of(LAND.faces[0].subtypes)
            .with(&numbers, Subtype::Knight);
        assert_eq!(
            land.iter(&numbers).collect::<Vec<_>>(),
            vec![Subtype::Island, Subtype::Knight]
        );
        assert!(numbers.has(Subtype::Knight).of(land) && !numbers.has(Subtype::Human).of(land));
    }

    /// CR 702.73a: a changeling is every creature type, though the game numbers none, and
    /// keeps them while it is a creature (CR 205.3c).
    #[test]
    fn a_changeling_is_every_creature_type() {
        let numbers = SubtypeNumbers::new(&[&SHAPESHIFTER, &LAND]).unwrap();
        let every = numbers.of(SHAPESHIFTER.faces[0].subtypes);
        assert!(every.contains(&numbers, Subtype::Elf));
        assert!(every.contains(&numbers, Subtype::TimeLord));
        assert!(!every.contains(&numbers, Subtype::Island));
        assert_eq!(every.of_types(&numbers, Types::LAND), GameSubtypes::NONE);
        assert_eq!(every.of_types(&numbers, Types::KINDRED), every);
    }

    /// A game whose cards name more subtypes than a set holds makes no numbers.
    #[test]
    fn too_many_subtypes() {
        let many: Vec<Subtype> = Subtype::ALL
            .iter()
            .copied()
            .filter(|s| s.card_type().contains(Types::CREATURE))
            .take(GAME_SUBTYPES + 1)
            .collect();
        let face = Face::EMPTY
            .types(Types::CREATURE)
            .subtypes(Subtypes::of(&many));
        let faces: &'static [Face] = Box::leak(Box::new([face]));
        let def: &'static CardDef = Box::leak(Box::new(card("Many", faces)));
        assert_eq!(SubtypeNumbers::new(&[def]).err(), Some(GAME_SUBTYPES + 1));
    }
}
