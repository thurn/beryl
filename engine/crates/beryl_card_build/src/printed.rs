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

/// A printed power or toughness: a number, or one with a star (`*`, `1+*`) where a
/// characteristic-defining ability sets it ([CR 208.2]): "Rather than a fixed number, some
/// creature cards have power and/or toughness that includes a star (*)." The star is 0 in the
/// definition, so `1+*` is 1: what the creature has once it has lost the ability. Nothing reads
/// a creature card's power off the battlefield, and the ability applies on it in layer 7a
/// ([CR 613.4a]): "Effects from characteristic-defining abilities that define power and/or
/// toughness are applied."
///
/// [CR 208.2]: https://yawgatog.com/resources/magic-rules/#R2082
/// [CR 613.4a]: https://yawgatog.com/resources/magic-rules/#R6134a
fn strength(value: &str) -> Option<i8> {
    if value == "*" {
        return Some(0);
    }
    if let Some(n) = value
        .strip_suffix("+*")
        .or_else(|| value.strip_prefix("*+"))
    {
        return n.parse().ok();
    }
    value.parse().ok()
}

/// Whether `face`'s rules text has changeling, a characteristic-defining ability that holds in
/// every zone ([CR 702.73a]): "Changeling is a characteristic-defining ability. 'Changeling'
/// means 'This object is every creature type.' This ability works everywhere, even outside the
/// game." So the face's printed subtypes are every creature type. A card file could not say
/// that, since it never types a subtype. The card file still writes `Keywords::CHANGELING`,
/// which its rules text shows.
///
/// [CR 702.73a]: https://yawgatog.com/resources/magic-rules/#R70273a
fn is_changeling(face: &Face) -> bool {
    let text = crate::complete::without_reminders(face.oracle_text.as_deref().unwrap_or_default());
    text.split(['\n', ','])
        .any(|words| words.trim().eq_ignore_ascii_case("changeling"))
}

/// The second set of characteristics a prototype card prints in its inset frame, read from
/// its Oracle text's first line, "Prototype {1}{U}{U} — 2/1" ([CR 719.1]): "The inset frame
/// contains the prototype keyword ability as well as a second set of power, toughness, and
/// mana cost characteristics." The `beryl_card::CastingOption` source, or none for a face
/// with no such line.
///
/// # Errors
///
/// A prototype line whose cost, power or toughness the engine cannot hold.
///
/// [CR 719.1]: https://yawgatog.com/resources/magic-rules/#R7191
fn prototype_source(face: &Face) -> Result<Option<String>, String> {
    let text = face.oracle_text.as_deref().unwrap_or_default();
    let Some(rest) = text
        .lines()
        .find_map(|line| line.strip_prefix("Prototype {"))
    else {
        return Ok(None);
    };
    let unreadable = || {
        format!(
            "{}: no prototype the engine can hold in {rest:?}",
            face.name
        )
    };
    let (cost, rest) = rest.split_once(" \u{2014} ").ok_or_else(unreadable)?;
    let stats = rest.split_whitespace().next().ok_or_else(unreadable)?;
    let (power, toughness) = stats.split_once('/').ok_or_else(unreadable)?;
    let power: i8 = power.parse().map_err(|_| unreadable())?;
    let toughness: i8 = toughness.parse().map_err(|_| unreadable())?;
    Ok(Some(format!(
        "beryl_card::CastingOption::prototype(beryl_card::mana(\"{{{cost}\"), {power}, {toughness})"
    )))
}

/// The generated shape, if it differs from the single face that the ordinary card builder
/// supplies. A meld result is a separate record with one face.
pub fn shape_source(record: &Record) -> Option<String> {
    let variant = match record.layout.as_str() {
        "transform" | "double_faced_token" => "Transforming".into(),
        "modal_dfc" => "Modal".into(),
        "split" => {
            let fuse = record.keywords.iter().any(|keyword| keyword == "Fuse");
            format!("Split {{ fuse: {fuse} }}")
        }
        "adventure" => "Adventure".into(),
        "prepare" => "Prepared".into(),
        "flip" => "Flip".into(),
        "meld" => {
            let meld = record.meld.as_ref()?;
            format!("Meld {{ with: {:?}, into: {:?} }}", meld.with, meld.into)
        }
        _ => return None,
    };
    Some(format!("beryl_card::definition::Shape::{variant}"))
}

/// The `beryl_card::Face` source holding a face's printed characteristics. The front of
/// a multi-face definition takes its card's name, so only a later face names itself.
///
/// # Errors
///
/// When the face has a supertype, card type or subtype the engine lacks, or a power,
/// toughness or loyalty that is no number, naming the face and what it has.
pub fn face_source(face: &Face, index: usize, subtypes: &Subtypes) -> Result<String, String> {
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
    let of = (!variants.is_empty())
        .then(|| format!("beryl_card::Subtypes::of(&[{}])", variants.join(", ")));
    match (is_changeling(face), of) {
        (true, of) => {
            let every = "beryl_card::Subtypes::every(beryl_card::Types::CREATURE)";
            let all = of.map_or(every.to_string(), |of| format!("{every}.union({of})"));
            write!(out, "\n    .subtypes({all})").unwrap();
        }
        (false, Some(of)) => write!(out, "\n    .subtypes({of})").unwrap(),
        (false, None) => {}
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
    if let Some(defense) = &face.defense {
        let value: u8 = defense
            .parse()
            .map_err(|_| format!("{name}: no defense the engine can hold in {defense}"))?;
        write!(out, "\n    .defense({value})").unwrap();
    }
    // A card file writes no casting option of a prototype card: the store's are complete.
    if let Some(prototype) = prototype_source(face)? {
        write!(out, "\n    .casting(&[{prototype}])").unwrap();
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
            // A flip card's "color and mana cost don't change if the permanent is flipped"
            // (CR 710.1c, https://yawgatog.com/resources/magic-rules/#R7101c).
            let flipped;
            let face = if i == 1 && record.layout == "flip" {
                let front = &record.faces[0];
                flipped = Face {
                    mana_cost: front.mana_cost.clone(),
                    colors: front.colors.clone(),
                    color_indicator: front.color_indicator.clone(),
                    ..face.clone()
                };
                &flipped
            } else {
                face
            };
            let mut source = face_source(face, i, subtypes)?;
            if i == 1
                && let Some(shape) = shape_source(record)
            {
                write!(
                    source,
                    "\n    .for_shape({shape}, beryl_card::FaceIx::BACK)"
                )
                .unwrap();
            }
            // The front of a split or preparation card has a name of its own.
            if record.faces.len() == 1
                || (i == 0 && matches!(record.layout.as_str(), "split" | "prepare"))
            {
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
