//! Printed characteristics used by the manual table.

use super::{Face, Record, TokenLink};

/// The default battlefield row for a card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row3 {
    Creatures,
    Others,
    Lands,
}

fn type_line(record: &Record) -> &str {
    record
        .faces
        .first()
        .map_or("", |face| face.type_line.as_str())
}

/// Reads the words before the long dash, where the card types and supertypes
/// appear. [CR 205.4a] says that “A card’s supertypes are printed directly
/// before its card types.”
///
/// [CR 205.4a]: https://yawgatog.com/resources/magic-rules/#R2054a
fn types(type_line: &str) -> impl Iterator<Item = &str> {
    type_line
        .split('—')
        .next()
        .unwrap_or_default()
        .split_whitespace()
}

pub fn is_land(record: &Record) -> bool {
    is_land_type_line(type_line(record))
}

pub fn is_land_type_line(type_line: &str) -> bool {
    types(type_line).any(|word| word == "Land")
}

pub fn is_creature(record: &Record) -> bool {
    is_creature_type_line(type_line(record))
}

pub fn is_creature_type_line(type_line: &str) -> bool {
    types(type_line).any(|word| word == "Creature")
}

pub fn is_planeswalker(record: &Record) -> bool {
    is_planeswalker_type_line(type_line(record))
}

pub fn is_planeswalker_type_line(type_line: &str) -> bool {
    types(type_line).any(|word| word == "Planeswalker")
}

pub fn is_permanent(record: &Record) -> bool {
    is_permanent_type_line(type_line(record))
}

/// [CR 110.4] says “There are six permanent types: artifact, battle, creature,
/// enchantment, land, and planeswalker.”
///
/// [CR 110.4]: https://yawgatog.com/resources/magic-rules/#R1104
pub fn is_permanent_type_line(type_line: &str) -> bool {
    types(type_line).any(|word| {
        matches!(
            word,
            "Artifact" | "Battle" | "Creature" | "Enchantment" | "Land" | "Planeswalker"
        )
    })
}

pub fn is_legendary(record: &Record) -> bool {
    is_legendary_type_line(type_line(record))
}

pub fn is_legendary_type_line(type_line: &str) -> bool {
    types(type_line).any(|word| word == "Legendary")
}

pub fn is_attachment(record: &Record) -> bool {
    type_line(record)
        .split_once('—')
        .is_some_and(|(_, subtypes)| {
            subtypes
                .split_whitespace()
                .any(|word| matches!(word, "Aura" | "Equipment" | "Fortification"))
        })
}

pub fn row(record: &Record) -> Row3 {
    if is_creature(record) {
        Row3::Creatures
    } else if is_land(record) {
        Row3::Lands
    } else {
        Row3::Others
    }
}

/// The manual table displays a printed non-number as zero until a player sets
/// the value.
pub fn printed_stats(face: &Face) -> Option<(i32, i32)> {
    let power = face.power.as_deref()?.parse().unwrap_or(0);
    let toughness = face.toughness.as_deref()?.parse().unwrap_or(0);
    Some((power, toughness))
}

/// Apply the manual override, then adjust both stats by the counter counts.
/// [CR 122.1a] says “Similarly, -X/-Y counters subtract from power and
/// toughness.”
///
/// [CR 122.1a]: https://yawgatog.com/resources/magic-rules/#R1221a
pub fn shown_stats(
    printed: Option<(i32, i32)>,
    manual: Option<(i32, i32)>,
    plus: i32,
    minus: i32,
) -> Option<(i32, i32)> {
    let (power, toughness) = manual.or(printed)?;
    let change = i64::from(plus) - i64::from(minus);
    let adjust = |stat| {
        let adjusted = i64::from(stat) + change;
        i32::try_from(adjusted).unwrap_or(if adjusted < 0 { i32::MIN } else { i32::MAX })
    };
    Some((adjust(power), adjust(toughness)))
}

/// The counters printed on the face. [CR 306.5b] says “equal to its printed
/// loyalty number,” and [CR 310.4b] says “equal to its printed defense number.”
///
/// [CR 306.5b]: https://yawgatog.com/resources/magic-rules/#R3065b
/// [CR 310.4b]: https://yawgatog.com/resources/magic-rules/#R3104b
pub fn starting_counters(face: &Face) -> Option<(&'static str, u16)> {
    [("loyalty", &face.loyalty), ("defense", &face.defense)]
        .into_iter()
        .find_map(|(name, printed)| Some((name, printed.as_deref()?.parse().ok()?)))
}

/// The manual table plays a face tapped when its Oracle text has an
/// enters-tapped sentence without “unless.” Ignore parenthesized reminder text.
pub fn enters_tapped(face: &Face) -> bool {
    let Some(text) = face.oracle_text.as_deref() else {
        return false;
    };
    let mut depth = 0_usize;
    let mut rules = String::new();
    for character in text.chars() {
        match character {
            '(' => {
                if depth == 0 {
                    rules.push(' ');
                }
                depth += 1;
            }
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => rules.push(character.to_ascii_lowercase()),
            _ => {}
        }
    }
    rules.split(['.', '!', '?', '\n']).any(|sentence| {
        let words: Vec<_> = sentence
            .split(|character: char| !character.is_alphabetic())
            .filter(|word| !word.is_empty())
            .collect();
        !words.contains(&"unless")
            && (words.windows(2).any(|words| words == ["enters", "tapped"])
                || words
                    .windows(4)
                    .any(|words| words == ["enters", "the", "battlefield", "tapped"]))
    })
}

/// The mana value of the cost printed on this face. [CR 202.3e] says “X is
/// treated as 0 while the object is not on the stack,” and [CR 202.3f] says
/// “use the largest component of each hybrid symbol.”
///
/// [CR 202.3e]: https://yawgatog.com/resources/magic-rules/#R2023e
/// [CR 202.3f]: https://yawgatog.com/resources/magic-rules/#R2023f
pub fn mana_value(face: &Face) -> u32 {
    face.mana_cost
        .as_deref()
        .unwrap_or_default()
        .split('{')
        .filter_map(|part| part.split_once('}').map(|(symbol, _)| symbol))
        .map(|symbol| {
            symbol
                .split('/')
                .map(|component| match component {
                    "X" => 0,
                    _ => component.parse::<u32>().unwrap_or(1),
                })
                .max()
                .unwrap_or(0)
        })
        .sum()
}

pub fn keywords(record: &Record) -> &[String] {
    &record.keywords
}

pub fn has_keyword(record: &Record, name: &str) -> bool {
    let lowercase = name.to_lowercase();
    keywords(record)
        .iter()
        .any(|keyword| keyword.to_lowercase() == lowercase)
}

pub fn tokens_made(record: &Record) -> &[TokenLink] {
    &record.tokens
}

#[cfg(test)]
mod tests;
