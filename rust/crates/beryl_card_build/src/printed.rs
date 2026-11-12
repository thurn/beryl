//! A face's printed characteristics, generated from its card store record (plan K2: "Printed
//! characteristics are generated from Scryfall for the whole pool into the card store", since
//! "Nobody types a cost or type line"). A card file's face starts from the constant written
//! here and adds its abilities.

use std::fmt::Write as _;

use beryl_card_store::{Face, Record};

/// The supertypes the engine knows
/// ([CR 205.4a](https://yawgatog.com/resources/magic-rules/#R2054a)): "An object can have one
/// or more supertypes."
const SUPERTYPES: [(&str, &str); 2] = [("Basic", "BASIC"), ("Legendary", "LEGENDARY")];

/// The card types the engine knows
/// ([CR 205.2a](https://yawgatog.com/resources/magic-rules/#R2052a)): "The card types are artifact, battle, conspiracy, creature, dungeon, enchantment, instant,
/// kindred, land, phenomenon, plane, planeswalker, scheme, sorcery, and vanguard."
const TYPES: [(&str, &str); 7] = [
    ("Artifact", "ARTIFACT"),
    ("Creature", "CREATURE"),
    ("Enchantment", "ENCHANTMENT"),
    ("Instant", "INSTANT"),
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

/// The variants of `beryl_card`'s `Subtype`, read from its source: the subtypes a face may
/// have until the catalogs generate that enum.
pub struct Subtypes(Vec<String>);

impl Subtypes {
    /// The variants of `pub enum Subtype` in `source`, `beryl_card`'s `characteristics.rs`.
    ///
    /// # Errors
    ///
    /// When the source has no such enum.
    pub fn parse(source: &str) -> Result<Self, String> {
        let body = source
            .split_once("pub enum Subtype {")
            .and_then(|(_, rest)| rest.split_once("\n}"))
            .ok_or("beryl_card's characteristics.rs has no `pub enum Subtype`")?
            .0;
        let variants = body
            .lines()
            .map(str::trim)
            .filter(|line| !line.starts_with("//") && !line.starts_with('#'))
            .filter_map(|line| line.strip_suffix(','))
            .map(str::to_string)
            .collect();
        Ok(Self(variants))
    }

    /// The variant spelling `subtype`, a word or words of a type line.
    fn variant(&self, subtype: &str) -> Option<&str> {
        let spelled: String = subtype
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        self.0.iter().find(|v| **v == spelled).map(String::as_str)
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
    // A type line is supertypes and card types, then a dash and subtypes
    // ([CR 205.1](https://yawgatog.com/resources/magic-rules/#R2051)): "It contains the
    // card's card type(s). It also contains the card's subtype(s) and supertype(s), if
    // applicable."
    let (types, subtype_words) = face
        .type_line
        .split_once(" — ")
        .unwrap_or((&face.type_line, ""));
    let mut supers = Vec::new();
    let mut cards = Vec::new();
    // "Token" and "Emblem" say what the object is, not a type it has (CR 111.1, CR 114.1).
    for word in types
        .split(' ')
        .filter(|w| !w.is_empty() && *w != "Token" && *w != "Emblem")
    {
        if SUPERTYPES.iter().any(|(s, _)| *s == word) {
            supers.push(word);
        } else if TYPES.iter().any(|(t, _)| *t == word) {
            cards.push(word);
        } else {
            return Err(format!(
                "{name}: the engine has no supertype or card type {word}"
            ));
        }
    }
    if let Some(s) = flags("Supertypes", &SUPERTYPES, &supers) {
        write!(out, "\n    .supertypes({s})").unwrap();
    }
    if let Some(t) = flags("Types", &TYPES, &cards) {
        write!(out, "\n    .types({t})").unwrap();
    }
    let mut variants = Vec::new();
    for word in subtype_words.split(' ').filter(|w| !w.is_empty()) {
        let variant = subtypes.variant(word).ok_or(format!(
            "{name}: beryl_card's Subtype has no {word}: add it there"
        ))?;
        variants.push(format!("beryl_card::Subtype::{variant}"));
    }
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
            let source = face_source(face, i, &record.layout, subtypes)?;
            Ok((face.name.clone(), source))
        })
        .collect()
}
