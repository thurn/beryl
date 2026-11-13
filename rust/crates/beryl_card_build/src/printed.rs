//! A face's printed characteristics, generated from its card store record. A card file's face starts from the constant written
//! here and adds its abilities.

use std::fmt::Write as _;

use beryl_card_store::{Face, Record};

use crate::catalogs::Catalogs;

/// The supertypes the engine knows
/// ([CR 205.4a](https://yawgatog.com/resources/magic-rules/#R2054a)): "An object can have one
/// or more supertypes."
pub const SUPERTYPES: [(&str, &str); 4] = [
    ("Basic", "BASIC"),
    ("Legendary", "LEGENDARY"),
    ("Snow", "SNOW"),
    ("World", "WORLD"),
];

/// The card types the engine knows
/// ([CR 205.2a](https://yawgatog.com/resources/magic-rules/#R2052a)): "The card types are artifact, battle, conspiracy, creature, dungeon, enchantment, instant,
/// kindred, land, phenomenon, plane, planeswalker, scheme, sorcery, and vanguard."
pub const TYPES: [(&str, &str); 10] = [
    ("Artifact", "ARTIFACT"),
    ("Battle", "BATTLE"),
    ("Conspiracy", "CONSPIRACY"),
    ("Creature", "CREATURE"),
    ("Enchantment", "ENCHANTMENT"),
    ("Instant", "INSTANT"),
    ("Kindred", "KINDRED"),
    ("Land", "LAND"),
    ("Planeswalker", "PLANESWALKER"),
    ("Sorcery", "SORCERY"),
];

/// The colors by Scryfall's letters
/// ([CR 105.1](https://yawgatog.com/resources/magic-rules/#R1051)): "There are five colors in the Magic game: white, blue, black, red, and green."
const COLORS: [(&str, &str); 5] = [
    ("W", "WHITE"),
    ("U", "BLUE"),
    ("B", "BLACK"),
    ("R", "RED"),
    ("G", "GREEN"),
];

/// The subtypes a face may have, each a name as a type line prints it with its variant of
/// `beryl_card`'s `Subtype`, which the catalogs generate ([`crate::catalogs`]).
pub struct Subtypes(Vec<(String, String)>);

impl Subtypes {
    /// Every subtype of the catalogs.
    #[must_use]
    pub fn of_catalogs(catalogs: &Catalogs) -> Self {
        Self(
            catalogs
                .subtypes
                .iter()
                .map(|s| (s.name.clone(), s.variant.clone()))
                .collect(),
        )
    }

    /// The subtypes `names` name, for a test.
    #[must_use]
    pub fn named(names: &[&str]) -> Self {
        Self(
            names
                .iter()
                .map(|n| ((*n).to_string(), crate::catalogs::variant(n)))
                .collect(),
        )
    }

    /// The variant of the subtype a type line prints as `name`.
    fn variant(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// The variants of the subtypes after a type line's dash. A creature type may be two
    /// words ([CR 205.3b](https://yawgatog.com/resources/magic-rules/#R2053b)): "Subtypes of
    /// creature cards are one or two words and are listed after a long dash." So two words
    /// that name one are read as it before either alone.
    ///
    /// # Errors
    ///
    /// The first word, or pair, that names no subtype.
    fn variants<'a>(&'a self, words: &'a str) -> Result<Vec<&'a str>, &'a str> {
        let words: Vec<&str> = words.split(' ').filter(|w| !w.is_empty()).collect();
        let mut found = Vec::new();
        let mut i = 0;
        while i < words.len() {
            let pair = words.get(i + 1).map(|next| format!("{} {next}", words[i]));
            if let Some(v) = pair.as_deref().and_then(|p| self.variant(p)) {
                found.push(v);
                i += 2;
            } else {
                found.push(self.variant(words[i]).ok_or(words[i])?);
                i += 1;
            }
        }
        Ok(found)
    }
}

/// A type line read as the engine's supertypes, card types and `Subtype` variants.
#[derive(Debug, PartialEq, Eq)]
pub struct TypeLine<'a> {
    pub supertypes: Vec<&'a str>,
    pub types: Vec<&'a str>,
    pub subtypes: Vec<&'a str>,
}

impl<'a> TypeLine<'a> {
    /// Reads `line`. A type line is supertypes and card types, then a dash and subtypes
    /// ([CR 205.1](https://yawgatog.com/resources/magic-rules/#R2051)): "It contains the
    /// card's card type(s). It also contains the card's subtype(s) and supertype(s), if
    /// applicable."
    ///
    /// # Errors
    ///
    /// The first word that is no supertype, card type or subtype the engine has.
    pub fn parse(line: &'a str, subtypes: &'a Subtypes) -> Result<Self, String> {
        let (types, subtype_words) = line.split_once(" — ").unwrap_or((line, ""));
        let mut read = TypeLine {
            supertypes: Vec::new(),
            types: Vec::new(),
            subtypes: Vec::new(),
        };
        // "Token" and "Emblem" say what the object is, not a type it has (CR 111.1, CR 114.1).
        for word in types
            .split(' ')
            .filter(|w| !w.is_empty() && *w != "Token" && *w != "Emblem")
        {
            if SUPERTYPES.iter().any(|(s, _)| *s == word) {
                read.supertypes.push(word);
            } else if TYPES.iter().any(|(t, _)| *t == word) {
                read.types.push(word);
            } else {
                return Err(format!("the engine has no supertype or card type {word}"));
            }
        }
        read.subtypes = subtypes
            .variants(subtype_words)
            .map_err(|word| format!("no subtype {word} in data/catalogs.json: run `just cards`"))?;
        Ok(read)
    }
}

/// The flags of `table` named by `words`, as `beryl_card::<kind>::A.union(...)`; none when
/// `words` is empty.
fn flags(kind: &str, table: &[(&str, &str)], words: &[&str]) -> Option<String> {
    let mut found = words.iter().filter_map(|word| {
        let (_, flag) = table.iter().find(|(name, _)| name == word)?;
        Some(format!("beryl_card::{kind}::{flag}"))
    });
    let first = found.next()?;
    Some(found.fold(first, |all, next| format!("{all}.union({next})")))
}

/// A printed power or toughness: a number, or `*` where a characteristic-defining ability
/// sets it, which is 0 in the definition. Nothing reads a creature card's power off the
/// battlefield, and the ability applies on it in layer 7a
/// ([CR 613.4a](https://yawgatog.com/resources/magic-rules/#R6134a)): "Effects from
/// characteristic-defining abilities that define power and/or toughness are applied."
fn strength(value: &str) -> Option<i8> {
    if value == "*" {
        return Some(0);
    }
    value.parse().ok()
}

/// The source of a `beryl_card::Face` constant holding `face`'s printed characteristics: the
/// face at `index` of a card or token of `layout`. A front face takes its card's name, so
/// only a later face names itself; the back face of a transforming card is never cast
/// ([CR 712.8](https://yawgatog.com/resources/magic-rules/#R7128)): "A double-faced spell is
/// cast with its front face up by default."
///
/// # Errors
///
/// When the face has a supertype, card type or subtype the engine lacks, or a power,
/// toughness or loyalty that is no number, naming the face and what it has.
pub fn face_source(
    face: &Face,
    index: usize,
    layout: &str,
    subtypes: &Subtypes,
) -> Result<String, String> {
    let name = &face.name;
    let mut out = String::from("beryl_card::Face::EMPTY");
    if index > 0 {
        write!(out, "\n    .named({name:?})").unwrap();
    }
    if let Some(cost) = face.mana_cost.as_deref().filter(|c| !c.is_empty()) {
        write!(out, "\n    .cost(beryl_card::mana({cost:?}))").unwrap();
    }
    let TypeLine {
        supertypes: supers,
        types: cards,
        subtypes: variants,
    } = TypeLine::parse(&face.type_line, subtypes).map_err(|e| format!("{name}: {e}"))?;
    if let Some(s) = flags("Supertypes", &SUPERTYPES, &supers) {
        write!(out, "\n    .supertypes({s})").unwrap();
    }
    if let Some(t) = flags("Types", &TYPES, &cards) {
        write!(out, "\n    .types({t})").unwrap();
    }
    let variants: Vec<String> = variants
        .into_iter()
        .map(|variant| format!("beryl_card::Subtype::{variant}"))
        .collect();
    if !variants.is_empty() {
        let of = variants.join(", ");
        write!(out, "\n    .subtypes(beryl_card::Subtypes::of(&[{of}]))").unwrap();
    }
    // A color indicator sets a face's color where its cost cannot
    // ([CR 202.2e](https://yawgatog.com/resources/magic-rules/#R2022e)): "That object is each
    // color denoted by that color indicator."
    let colors: Vec<&str> = face
        .color_indicator
        .as_ref()
        .unwrap_or(&face.colors)
        .iter()
        .map(String::as_str)
        .collect();
    if let Some(c) = flags("Colors", &COLORS, &colors) {
        write!(out, "\n    .colors({c})").unwrap();
    }
    if let (Some(power), Some(toughness)) = (&face.power, &face.toughness) {
        let (Some(p), Some(t)) = (strength(power), strength(toughness)) else {
            return Err(format!(
                "{name}: no power and toughness the engine can hold in {power}/{toughness}"
            ));
        };
        write!(out, "\n    .power_toughness({p}, {t})").unwrap();
    }
    if let Some(loyalty) = &face.loyalty {
        let value: u8 = loyalty
            .parse()
            .map_err(|_| format!("{name}: no loyalty the engine can hold in {loyalty}"))?;
        write!(out, "\n    .loyalty({value})").unwrap();
    }
    if index == 1 && layout == "transform" {
        out.push_str("\n    .transformed()");
    }
    Ok(out)
}

/// Each face of `record` as a `(name, source)` pair, front first.
///
/// # Errors
///
/// As [`face_source`].
pub fn faces(record: &Record, subtypes: &Subtypes) -> Result<Vec<(String, String)>, String> {
    record
        .faces
        .iter()
        .enumerate()
        .map(|(i, face)| {
            let mut source = face_source(face, i, &record.layout, subtypes)?;
            // A card of one face carries its name, which `Face::card` builds the card under.
            if record.faces.len() == 1 {
                source = source.replacen(
                    "beryl_card::Face::EMPTY",
                    &format!("beryl_card::Face::EMPTY\n    .named({:?})", face.name),
                    1,
                );
            }
            Ok((face.name.clone(), source))
        })
        .collect()
}
