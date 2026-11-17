//! Words: the color words and subtypes a card's text names, which a text-changing effect can
//! change ([CR 612.1]): "Some continuous effects change an object’s text. This can apply to
//! any words or symbols printed on that object, but generally affects only that object’s
//! rules text (which appears in its text box) and/or the text that appears in its type line."
//!
//! A word is a value of a type with a private field, and the only way to its value is
//! `read`, which takes the [`Words`] of the object whose text names it. A reader that forgot
//! an object's text changes would not compile:
//!
//! ```compile_fail
//! use beryl_card::{ColorWords, Colors};
//!
//! // A color word is not a color until it is read through an object's words.
//! let red: Colors = ColorWords::new(Colors::RED).0;
//! ```
//!
//! ```compile_fail
//! use beryl_card::{Filter, Subtype};
//!
//! // Nor is a subtype a filter names.
//! if let Filter::SubtypeWord(word) = Filter::Subtype(Subtype::Aura) {
//!     let aura: Subtype = word.0;
//! }
//! ```
//!
//! Only words used as what they are change ([CR 612.2]): "A text-changing effect changes
//! only those words that are used in the correct way (for example, a Magic color word being
//! used as a color word, a land type word used as a land type, or a creature type word used
//! as a creature type)." So a card's name is a plain string, and a mana symbol is a
//! [`crate::ManaColor`], never a word.
//!
//! [CR 612.1]: https://yawgatog.com/resources/magic-rules/#R6121
//! [CR 612.2]: https://yawgatog.com/resources/magic-rules/#R6122

use crate::characteristics::{BasicLandTypes, Colors, Keywords, Subtype, Subtypes};

/// The text changes on one object: for each color word and each basic land type word, the
/// word it reads as instead. Several text-changing effects compose into one `Words`, each
/// applied to what the earlier ones left.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Words {
    /// What each color word reads as, in [`Colors::EACH`]'s order.
    colors: [Colors; 5],
    /// What each basic land type word reads as, in [`BasicLandTypes::SUBTYPES`]' order.
    land_types: [BasicLandWord; 5],
}

/// No changes: the filler of a fixed-capacity list's unused places.
impl Default for Words {
    fn default() -> Self {
        Words::NONE
    }
}

impl Words {
    /// No changes: each word reads as itself.
    pub const NONE: Words = Words {
        colors: Colors::EACH,
        land_types: BasicLandWord::EACH,
    };

    /// These changes, then every instance of the color word `from` replaced with `to`: a
    /// word that read as `from` reads as `to`. Each is one color.
    #[must_use]
    pub fn changing_color(mut self, from: Colors, to: Colors) -> Words {
        debug_assert!(from.0.is_power_of_two() && to.0.is_power_of_two());
        for color in &mut self.colors {
            if *color == from {
                *color = to;
            }
        }
        self
    }

    /// These changes, then every instance of the basic land type word `from` replaced with
    /// `to`.
    fn changing_land_type(mut self, from: BasicLandWord, to: BasicLandWord) -> Words {
        for reads in &mut self.land_types {
            if *reads == from {
                *reads = to;
            }
        }
        self
    }

    /// What one color reads as.
    fn color(&self, color: Colors) -> Colors {
        let mut read = Colors::NONE;
        for (i, &each) in Colors::EACH.iter().enumerate() {
            if color.contains(each) {
                read = read.union(self.colors[i]);
            }
        }
        read
    }

    /// What one subtype reads as.
    fn subtype(&self, subtype: Subtype) -> Subtype {
        BasicLandWord::of_subtype(subtype).map_or(subtype, |word| {
            self.land_types[usize::from(word.0)].subtype()
        })
    }
}

impl Words {
    /// These changes, then `change`.
    #[must_use]
    pub fn changing(self, change: TextChange) -> Words {
        match change {
            TextChange::Color { from, to } => self.changing_color(from, to),
            TextChange::LandType { from, to } => self.changing_land_type(from, to),
        }
    }

    /// Whether every word reads as printed.
    #[inline]
    pub fn is_none(&self) -> bool {
        !self.changes_subtypes() && self.colors == Colors::EACH
    }

    /// Whether some subtype word reads as another: the type line, a land type or a landwalk
    /// may read otherwise than printed.
    #[inline]
    pub fn changes_subtypes(&self) -> bool {
        self.land_types != BasicLandWord::EACH
    }

    /// The landwalk keywords among `keywords` as these words read them: "forestwalk" whose
    /// "Forest" reads as "Island" is islandwalk ([CR 702.14a]): "Landwalk is a generic term
    /// that appears within an object's rules text as “[type]walk,” where [type] is usually a
    /// land type". The other keywords are kept.
    ///
    /// [CR 702.14a]: https://yawgatog.com/resources/magic-rules/#R70214a
    #[inline]
    #[must_use]
    pub fn landwalk(&self, keywords: Keywords) -> Keywords {
        if !self.changes_subtypes() || !keywords.intersects(Keywords::ANY_LANDWALK) {
            return keywords;
        }
        self.landwalk_changed(keywords)
    }

    /// [`Words::landwalk`] when some subtype word reads otherwise and `keywords` has a
    /// landwalk.
    #[cold]
    #[inline(never)]
    fn landwalk_changed(&self, keywords: Keywords) -> Keywords {
        let mut read = keywords.minus(Keywords::ANY_LANDWALK);
        for (walk, land) in Keywords::LANDWALK {
            if keywords.contains(walk) {
                let reads = self.subtype(land);
                if let Some(&(other, _)) = Keywords::LANDWALK.iter().find(|&&(_, s)| s == reads) {
                    read = read.union(other);
                }
            }
        }
        read
    }
}

/// The color words of a [`Words`] alone, in two bytes: for each color in [`Colors::EACH`]'s
/// order, three bits naming the color it reads as. What an effect that outlives the object
/// whose text it was made from keeps of that text, where a whole [`Words`] has no room.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ColorChanges(u16);

impl ColorChanges {
    /// Each color word reads as itself.
    pub const NONE: ColorChanges = ColorChanges(0b100_011_010_001_000);
}

impl Words {
    /// Its color words alone.
    pub fn color_changes(&self) -> ColorChanges {
        let mut packed = 0u16;
        for (i, reads) in self.colors.iter().enumerate() {
            let n = Colors::EACH.iter().position(|c| c == reads).unwrap_or(i);
            packed |= u16::try_from(n).unwrap_or(0) << (3 * i);
        }
        ColorChanges(packed)
    }

    /// Words whose color words read as `changes` say, and whose subtypes read as printed.
    pub fn of_color_changes(changes: ColorChanges) -> Words {
        let mut words = Words::NONE;
        for (i, color) in words.colors.iter_mut().enumerate() {
            let n = usize::from((changes.0 >> (3 * i)) & 0b111);
            *color = Colors::EACH.get(n).copied().unwrap_or(*color);
        }
        words
    }
}

/// One change of an object's text ([CR 612.1]): every instance of one color word replaced
/// with another, or of one basic land type word with another.
///
/// [CR 612.1]: https://yawgatog.com/resources/magic-rules/#R6121
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextChange {
    /// Each a single color.
    Color { from: Colors, to: Colors },
    LandType {
        from: BasicLandWord,
        to: BasicLandWord,
    },
}

/// One word from [`BasicLandTypes::SUBTYPES`], stored as its index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BasicLandWord(u8);

impl BasicLandWord {
    const EACH: [Self; 5] = [Self(0), Self(1), Self(2), Self(3), Self(4)];

    /// The word for `subtype`, if it names a basic land type.
    pub fn of_subtype(subtype: Subtype) -> Option<Self> {
        BasicLandTypes::SUBTYPES
            .iter()
            .zip(Self::EACH)
            .find_map(|(&(_, land), word)| (land == subtype).then_some(word))
    }

    /// The subtype this word names as printed.
    fn subtype(self) -> Subtype {
        BasicLandTypes::SUBTYPES[usize::from(self.0)].1
    }
}

/// A word a player may choose to replace, or to replace it with, as a text-changing effect
/// that names color words and basic land types begins: "replacing all instances of one color
/// word with another or one basic land type with another". Numbered for a catalog prompt:
/// the five colors in [`Colors::EACH`]'s order, then the five basic land types in
/// [`BasicLandTypes::SUBTYPES`]' order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextWord {
    Color(Colors),
    LandType(Subtype),
}

impl TextWord {
    /// How many there are.
    pub const COUNT: u8 = 10;

    /// The word numbered `n`.
    pub const fn of_number(n: u8) -> Option<TextWord> {
        let i = n as usize;
        if i < 5 {
            Some(TextWord::Color(Colors::EACH[i]))
        } else if i < 10 {
            Some(TextWord::LandType(BasicLandTypes::SUBTYPES[i - 5].1))
        } else {
            None
        }
    }

    /// Its number. A subtype outside the five basic land types returns [`TextWord::COUNT`].
    pub const fn number(self) -> u8 {
        match self {
            TextWord::Color(color) => color.0.trailing_zeros() as u8,
            TextWord::LandType(subtype) => {
                let mut i = 0;
                while i < 5 {
                    if BasicLandTypes::SUBTYPES[i].1 as u16 == subtype as u16 {
                        return 5 + i as u8;
                    }
                    i += 1;
                }
                TextWord::COUNT
            }
        }
    }

    /// What it is called: "Red", "Forest".
    pub fn name(self) -> &'static str {
        match self {
            TextWord::Color(color) => (Colors::EACH.iter().zip(Colors::NAMES))
                .find_map(|(&c, name)| (c == color).then_some(name))
                .unwrap_or("No color"),
            TextWord::LandType(subtype) => subtype.name(),
        }
    }

    /// The change that replaces this word with `to`, if both are words of one kind and they
    /// differ: one color word can only be replaced with another color word, and a word with
    /// a different word (Mind Bend's ruling: "It can't change a word to the same word.").
    pub fn to(self, to: TextWord) -> Option<TextChange> {
        match (self, to) {
            (TextWord::Color(from), TextWord::Color(to)) if from != to => {
                Some(TextChange::Color { from, to })
            }
            (TextWord::LandType(from), TextWord::LandType(to)) if from != to => {
                Some(TextChange::LandType {
                    from: BasicLandWord::of_subtype(from)?,
                    to: BasicLandWord::of_subtype(to)?,
                })
            }
            _ => None,
        }
    }
}

/// Color words: "red", "white and blue". Holds a set, since a card names several at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ColorWords(Colors);

impl ColorWords {
    /// The words for `colors`, as a card's text prints them.
    #[inline]
    pub const fn new(colors: Colors) -> ColorWords {
        ColorWords(colors)
    }

    /// The colors these words name on an object with `words`.
    #[inline]
    pub fn read(self, words: &Words) -> Colors {
        if words.colors == Colors::EACH {
            self.0
        } else {
            words.color(self.0)
        }
    }
}

/// A subtype word: "Aura", "Island", "Elk". The supported text changes replace basic land
/// type words. Other subtype words read as printed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SubtypeWord(Subtype);

impl SubtypeWord {
    /// The word for `subtype`, as a card's text prints it.
    #[inline]
    pub const fn new(subtype: Subtype) -> SubtypeWord {
        SubtypeWord(subtype)
    }

    /// The subtype this word names on an object with `words`.
    #[inline]
    pub fn read(self, words: &Words) -> Subtype {
        if words.changes_subtypes() {
            words.subtype(self.0)
        } else {
            self.0
        }
    }
}

/// The subtypes a type line prints, which are words of its text too ([CR 612.1], "the text
/// that appears in its type line"): a Forest whose text has "Forest" replaced with "Island"
/// is an Island.
///
/// [CR 612.1]: https://yawgatog.com/resources/magic-rules/#R6121
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SubtypeWords(Subtypes);

impl SubtypeWords {
    /// No subtypes.
    pub const NONE: SubtypeWords = SubtypeWords(Subtypes::NONE);

    /// The words for `subtypes`, as a type line prints them.
    #[inline]
    pub const fn new(subtypes: Subtypes) -> SubtypeWords {
        SubtypeWords(subtypes)
    }

    /// The subtypes these words name on an object with `words`.
    #[inline]
    pub fn read(self, words: &Words) -> Subtypes {
        if !words.changes_subtypes() {
            self.0
        } else {
            self.0
                .iter()
                .fold(Subtypes::NONE, |set, s| set.with(words.subtype(s)))
        }
    }
}

/// Basic land type words: "is a Mountain", "is a Plains and an Island".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LandTypeWords(BasicLandTypes);

impl LandTypeWords {
    /// The words for `types`, as a card's text prints them.
    #[inline]
    pub const fn new(types: BasicLandTypes) -> LandTypeWords {
        LandTypeWords(types)
    }

    /// The basic land types these words name on an object with `words`.
    #[inline]
    pub fn read(self, words: &Words) -> BasicLandTypes {
        if !words.changes_subtypes() {
            self.0
        } else {
            self.0
                .subtypes()
                .filter_map(|s| BasicLandTypes::of_subtype(words.subtype(s)))
                .fold(BasicLandTypes::NONE, BasicLandTypes::union)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BasicLandWord, ColorChanges, ColorWords, LandTypeWords, SubtypeWord, SubtypeWords,
        TextWord, Words,
    };
    use crate::characteristics::{BasicLandTypes, Colors, Keywords, Subtype, Subtypes};

    /// Without changes every word reads as printed.
    #[test]
    fn no_changes_read_as_printed() {
        let words = Words::NONE;
        let both = Colors::WHITE.union(Colors::BLUE);
        assert_eq!(ColorWords::new(both).read(&words), both);
        assert_eq!(SubtypeWord::new(Subtype::Aura).read(&words), Subtype::Aura);
        let line = Subtypes::of(&[Subtype::Human, Subtype::Knight]);
        assert_eq!(SubtypeWords::new(line).read(&words), line);
    }

    /// CR 612.1: a change reaches every instance of the word, in the text and the type line,
    /// and leaves the others.
    #[test]
    fn a_change_replaces_every_instance() {
        let forest = BasicLandWord::of_subtype(Subtype::Forest).unwrap();
        let island = BasicLandWord::of_subtype(Subtype::Island).unwrap();
        let words = Words::NONE
            .changing_color(Colors::RED, Colors::BLUE)
            .changing_land_type(forest, island);
        let red_white = Colors::RED.union(Colors::WHITE);
        assert_eq!(
            ColorWords::new(red_white).read(&words),
            Colors::BLUE.union(Colors::WHITE)
        );
        assert_eq!(
            SubtypeWord::new(Subtype::Forest).read(&words),
            Subtype::Island
        );
        assert_eq!(SubtypeWord::new(Subtype::Aura).read(&words), Subtype::Aura);
        let line = SubtypeWords::new(Subtypes::of(&[Subtype::Forest]));
        assert_eq!(line.read(&words), Subtypes::of(&[Subtype::Island]));
        let types = LandTypeWords::new(BasicLandTypes::FOREST.union(BasicLandTypes::PLAINS));
        assert_eq!(
            types.read(&words),
            BasicLandTypes::ISLAND.union(BasicLandTypes::PLAINS)
        );
    }

    /// Changes compose in order: red to blue then blue to green turns red into green, and a
    /// word that was blue before either is green too.
    #[test]
    fn changes_compose_in_order() {
        let words = Words::NONE
            .changing_color(Colors::RED, Colors::BLUE)
            .changing_color(Colors::BLUE, Colors::GREEN);
        assert_eq!(ColorWords::new(Colors::RED).read(&words), Colors::GREEN);
        assert_eq!(ColorWords::new(Colors::BLUE).read(&words), Colors::GREEN);
        let forest = BasicLandWord::of_subtype(Subtype::Forest).unwrap();
        let island = BasicLandWord::of_subtype(Subtype::Island).unwrap();
        let swamp = BasicLandWord::of_subtype(Subtype::Swamp).unwrap();
        let words = Words::NONE
            .changing_land_type(forest, island)
            .changing_land_type(island, swamp);
        assert_eq!(
            SubtypeWord::new(Subtype::Forest).read(&words),
            Subtype::Swamp
        );
        assert_eq!(
            SubtypeWord::new(Subtype::Island).read(&words),
            Subtype::Swamp
        );
    }

    /// A landwalk names a land type (CR 702.14a), so forestwalk whose Forest reads as Island
    /// is islandwalk, and the other keywords stay.
    #[test]
    fn a_landwalk_reads_through_its_land_type() {
        let forest = BasicLandWord::of_subtype(Subtype::Forest).unwrap();
        let island = BasicLandWord::of_subtype(Subtype::Island).unwrap();
        let words = Words::NONE.changing_land_type(forest, island);
        let printed = Keywords::FORESTWALK.union(Keywords::FLYING);
        assert_eq!(
            words.landwalk(printed),
            Keywords::ISLANDWALK.union(Keywords::FLYING)
        );
    }

    /// The ten words a text change chooses among are numbered, and only two words of one kind
    /// that differ make a change (Mind Bend's ruling: "It can't change a word to the same
    /// word.").
    #[test]
    fn words_are_replaced_by_words_of_their_kind() {
        for n in 0..TextWord::COUNT {
            let word = TextWord::of_number(n).unwrap();
            assert_eq!(word.number(), n);
            assert!(word.to(word).is_none());
        }
        let red = TextWord::Color(Colors::RED);
        let forest = TextWord::LandType(Subtype::Forest);
        let cave = TextWord::LandType(Subtype::Cave);
        assert!(red.to(forest).is_none());
        assert!(cave.to(forest).is_none());
        assert!(forest.to(cave).is_none());
        assert!(red.to(TextWord::Color(Colors::BLUE)).is_some());
        assert_eq!(forest.name(), "Forest");
    }

    /// A shield keeps the color words of a text in two bytes and reads them back the same.
    #[test]
    fn color_changes_round_trip() {
        assert_eq!(Words::NONE.color_changes(), ColorChanges::NONE);
        let words = Words::NONE.changing_color(Colors::RED, Colors::BLUE);
        let back = Words::of_color_changes(words.color_changes());
        assert_eq!(ColorWords::new(Colors::RED).read(&back), Colors::BLUE);
        assert_eq!(back, words);
    }
}
