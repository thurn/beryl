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

use crate::characteristics::{BasicLandTypes, Colors, Subtype, Subtypes};

/// The most subtype words one object's text changes can replace, each with another.
pub const SUBTYPE_CHANGES: usize = 4;

/// The text changes on one object: for each color word and each subtype it replaces, the
/// word it reads as instead. Several text-changing effects compose into one `Words`, each
/// applied to what the earlier ones left.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Words {
    /// What each color word reads as, in [`Colors::EACH`]'s order.
    colors: [Colors; 5],
    /// The first `changed` entries are subtype words with what each reads as.
    subtypes: [(Subtype, Subtype); SUBTYPE_CHANGES],
    changed: u8,
}

impl Words {
    /// No changes: each word reads as itself.
    pub const NONE: Words = Words {
        colors: Colors::EACH,
        subtypes: [(Subtype::ALL[0], Subtype::ALL[0]); SUBTYPE_CHANGES],
        changed: 0,
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

    /// These changes, then every instance of the subtype word `from` replaced with `to`, or
    /// `None` when that would need more than [`SUBTYPE_CHANGES`] entries.
    #[must_use]
    pub fn changing_subtype(mut self, from: Subtype, to: Subtype) -> Option<Words> {
        let changed = usize::from(self.changed);
        let mut replaced = false;
        for (word, reads) in &mut self.subtypes[..changed] {
            replaced |= *word == from;
            if *reads == from {
                *reads = to;
            }
        }
        if !replaced {
            *self.subtypes.get_mut(changed)? = (from, to);
            self.changed += 1;
        }
        Some(self)
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
        self.subtypes[..usize::from(self.changed)]
            .iter()
            .find(|&&(word, _)| word == subtype)
            .map_or(subtype, |&(_, reads)| reads)
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

/// A subtype word: "Aura", "Island", "Elk". A text change replaces a basic land type or a
/// creature type; any other subtype reads as itself, since no change names it.
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
        if words.changed == 0 {
            self.0
        } else {
            words.subtype(self.0)
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
        if words.changed == 0 {
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
        if words.changed == 0 {
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
    use super::{ColorWords, LandTypeWords, SubtypeWord, SubtypeWords, Words};
    use crate::characteristics::{BasicLandTypes, Colors, Subtype, Subtypes};

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
        let words = Words::NONE
            .changing_color(Colors::RED, Colors::BLUE)
            .changing_subtype(Subtype::Forest, Subtype::Island)
            .unwrap();
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
        let words = Words::NONE
            .changing_subtype(Subtype::Forest, Subtype::Island)
            .and_then(|w| w.changing_subtype(Subtype::Island, Subtype::Swamp))
            .unwrap();
        assert_eq!(
            SubtypeWord::new(Subtype::Forest).read(&words),
            Subtype::Swamp
        );
        assert_eq!(
            SubtypeWord::new(Subtype::Island).read(&words),
            Subtype::Swamp
        );
    }
}
