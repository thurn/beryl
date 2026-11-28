//! The English the templates share: numbers, articles, lists, mana symbols and the names of
//! characteristics.

use crate::characteristics::{Colors, Keywords, Supertypes, Types};
use crate::continuous::Protection;
use crate::ids::TurnStep;
use crate::mana::{ManaColor, ManaCost, ManaSet};
use crate::words::Words;

/// A number of things as Oracle text spells it: "a card", "two cards", up to ten, then
/// digits ("11 cards").
pub(super) fn number(n: u8) -> String {
    const WORDS: [&str; 11] = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
    ];
    WORDS
        .get(usize::from(n))
        .map_or_else(|| n.to_string(), |word| (*word).to_owned())
}

/// "Second", "third": a position counted from the top of a library
/// ([CR 401.7](https://yawgatog.com/resources/magic-rules/#R4017)): "If an effect causes a
/// player to put a card into a library 'Nth from the top,' ...".
pub(super) fn ordinal(n: u8) -> String {
    const WORDS: [&str; 11] = [
        "zeroth", "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth",
        "ninth", "tenth",
    ];
    WORDS
        .get(usize::from(n))
        .map_or_else(|| format!("{n}th"), |word| (*word).to_owned())
}

/// "a" or "an", by the sound of the word that follows: "an artifact", "a +1/+1 counter".
pub(super) fn article(word: &str) -> &'static str {
    match word.chars().next() {
        Some(c) if "aeiouAEIOU".contains(c) => "an",
        _ => "a",
    }
}

/// `word` with its article: "an Island".
pub(super) fn with_article(word: &str) -> String {
    format!("{} {word}", article(word))
}

/// `count` things: "a card", "two cards", "X cards".
pub(super) fn counted(count: u8, singular: &str, plural: &str) -> String {
    if count == 1 {
        with_article(singular)
    } else {
        format!("{} {plural}", number(count))
    }
}

/// The plural of a noun Oracle text uses: "creatures", "sorceries", "Equipment".
pub(super) fn plural(noun: &str) -> String {
    // A subtype is capitalized; the few that do not take an "s" are the ones the pool names.
    if matches!(noun, "Equipment" | "Fish" | "Sheep" | "Elk" | "Plains") {
        return noun.to_owned();
    }
    if let Some(stem) = noun.strip_suffix("ery") {
        return format!("{stem}eries");
    }
    if noun.ends_with('s') || noun.ends_with("ch") || noun.ends_with("sh") {
        return format!("{noun}es");
    }
    format!("{noun}s")
}

/// Items joined as a list: "a", "a or b", "a, b, or c".
pub(super) fn list(items: &[String], conjunction: &str) -> String {
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [first, second] => format!("{first} {conjunction} {second}"),
        [init @ .., last] => format!("{}, {conjunction} {last}", init.join(", ")),
    }
}

/// `text` with its first letter capitalized, as a sentence starts.
pub(super) fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// `text` with its first letter in lower case, as a clause inside a sentence starts. A word
/// that starts with two capitals ("X") or a symbol is left as it is.
pub(super) fn uncapitalized(text: &str) -> String {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(first), Some(second)) if first.is_uppercase() && second.is_lowercase() => first
            .to_lowercase()
            .chain(Some(second))
            .chain(chars)
            .collect(),
        _ => text.to_owned(),
    }
}

/// `text` as one sentence: capitalized, with a full stop unless it has one.
pub(super) fn sentence(text: &str) -> String {
    let text = capitalized(text.trim());
    if text.is_empty() || text.ends_with('.') || text.ends_with('"') {
        text
    } else {
        format!("{text}.")
    }
}

/// A mana cost in braces, X first, as Oracle text writes it: "{X}{2}{W}{W}", "{1}{W/B}{W/B}"
/// ([CR 107.4](https://yawgatog.com/resources/magic-rules/#R1074)): "The mana symbols are
/// {W}, {U}, {B}, {R}, {G}, and {C}; the numerical symbols {0}, {1}, {2}, {3}, {4}, and so
/// on; the variable symbol {X}; the hybrid symbols {W/U}, {W/B}, {U/B}, {U/R}, {B/R}, {B/G},
/// {R/G}, {R/W}, {G/W}, and {G/U}; ...". A cost of nothing is "{0}". Hybrid symbols come
/// before the other colored symbols, as most printed costs that have both write them
/// ("{2}{W/B}{U}").
pub fn mana_text(cost: ManaCost) -> String {
    let mut text = "{X}".repeat(usize::from(cost.x));
    if cost.generic > 0 || (cost.x == 0 && cost.symbols == [0; 6] && cost.hybrid.is_empty()) {
        text.push('{');
        text.push_str(&cost.generic.to_string());
        text.push('}');
    }
    for (colors, count) in cost.hybrid.pairs() {
        text.push_str(&hybrid_symbol(colors).repeat(usize::from(count)));
    }
    for (color, &count) in ManaColor::ALL.iter().zip(&cost.symbols) {
        text.push_str(&symbol(*color).repeat(usize::from(count)));
    }
    text
}

/// The symbol of a two-color hybrid pair of [`crate::Hybrid::PAIRS`], such as "{W/B}", its
/// halves in the order CR 107.4 lists the symbol.
///
/// # Panics
///
/// On a set of colors that is no pair.
fn hybrid_symbol(colors: u8) -> &'static str {
    const SYMBOLS: [&str; 10] = [
        "{W/U}", "{U/B}", "{B/R}", "{R/G}", "{G/W}", "{W/B}", "{U/R}", "{B/G}", "{R/W}", "{G/U}",
    ];
    SYMBOLS[crate::Hybrid::pair(colors).expect("a hybrid pair")]
}

/// The symbol of one mana of a color, or of colorless mana: "{U}", "{C}" (CR 107.4).
pub(super) fn symbol(color: ManaColor) -> &'static str {
    match color {
        ManaColor::White => "{W}",
        ManaColor::Blue => "{U}",
        ManaColor::Black => "{B}",
        ManaColor::Red => "{R}",
        ManaColor::Green => "{G}",
        ManaColor::Colorless => "{C}",
    }
}

/// What "add [mana]" adds, such as "{U}", "{C}{C}", "{W} or {U}", "{G}, {W}, or {U}", and
/// "one mana of any color" for all five colors. Where the set has several kinds, the player
/// chooses one ([CR 106.3](https://yawgatog.com/resources/magic-rules/#R1063)).
pub(super) fn mana_set(mana: ManaSet) -> String {
    let colors: Vec<ManaColor> = ManaColor::ALL
        .into_iter()
        .filter(|&color| mana.includes(color))
        .collect();
    let five = ManaColor::ALL
        .iter()
        .filter(|&&color| color != ManaColor::Colorless)
        .all(|&color| mana.includes(color));
    if five {
        return if mana.amount == 1 {
            "one mana of any color".to_owned()
        } else {
            format!("{} mana of any one color", number(mana.amount))
        };
    }
    let each: Vec<String> = colors
        .iter()
        .map(|&color| symbol(color).repeat(usize::from(mana.amount)))
        .collect();
    list(&each, "or")
}

/// A constant's name as words: `FIRST_STRIKE` is "first strike".
fn words_of(constant: &str) -> String {
    constant.to_lowercase().replace('_', " ")
}

/// The names of the members of a set, in the order the set's list writes them.
fn named<T: Copy>(set: T, all: &[(T, &str)], has: impl Fn(T, T) -> bool) -> Vec<String> {
    all.iter()
        .filter(|&&(member, _)| has(set, member))
        .map(|&(_, name)| words_of(name))
        .collect()
}

/// The card types of a set, in lower case: "artifact", "creature"
/// ([CR 205.2a](https://yawgatog.com/resources/magic-rules/#R2052a)).
pub(super) fn types(types: Types) -> Vec<String> {
    named(types, Types::NAMED, Types::contains)
}

/// The supertypes of a set, in lower case: "basic", "legendary"
/// ([CR 205.4a](https://yawgatog.com/resources/magic-rules/#R2054a)).
pub(super) fn supertypes(supertypes: Supertypes) -> Vec<String> {
    named(supertypes, Supertypes::NAMED, Supertypes::contains)
}

/// The colors of a set, in lower case: "white", "blue"
/// ([CR 105.1](https://yawgatog.com/resources/magic-rules/#R1051)).
pub(super) fn colors(colors: Colors) -> Vec<String> {
    named(colors, Colors::NAMED, Colors::contains)
}

/// A protection ability, "protection from red and from blue" or "protection from the color
/// of your choice" ([CR 702.16g]): "“Protection from [quality A] and from [quality B]” is
/// shorthand for “protection from [quality A]” and “protection from [quality B]”".
///
/// [CR 702.16g]: https://yawgatog.com/resources/magic-rules/#R70216g
pub(super) fn protection(protection: Protection, words: &Words) -> String {
    match protection {
        Protection::ColorWords(set) => {
            let from: Vec<String> = colors(set.read(words))
                .iter()
                .map(|c| format!("from {c}"))
                .collect();
            format!("protection {}", list(&from, "and"))
        }
        Protection::ChosenColor => "protection from the color of your choice".to_owned(),
    }
}

/// The keyword abilities of a set, in lower case, flash first and flying next, as Oracle
/// text tends to print them: "flash", "flying", "first strike"
/// ([CR 702.1](https://yawgatog.com/resources/magic-rules/#R7021)).
pub(super) fn keywords(keywords: Keywords) -> Vec<String> {
    let mut names = named(keywords, Keywords::NAMED, Keywords::contains);
    let rank = |name: &String| match name.as_str() {
        "flash" => 0,
        "flying" => 1,
        _ => 2,
    };
    names.sort_by_key(rank);
    names
}

/// A power or toughness change as Oracle prints it, a zero taking the other number's sign:
/// "+1/+1", "-2/-0" ([CR 613.4c](https://yawgatog.com/resources/magic-rules/#R6134c)).
pub(super) fn pt_change(power: i8, toughness: i8) -> String {
    let sign = |n: i8, other: i8| {
        if n > 0 || (n == 0 && other >= 0) {
            format!("+{n}")
        } else {
            format!("-{}", n.unsigned_abs())
        }
    };
    format!("{}/{}", sign(power, toughness), sign(toughness, power))
}

/// The name of a step or phase as a trigger says it, such as "upkeep" or "end step"
/// ([CR 500.1](https://yawgatog.com/resources/magic-rules/#R5001)).
pub(super) fn step_name(step: TurnStep) -> &'static str {
    match step {
        TurnStep::Untap => "untap step",
        TurnStep::Upkeep => "upkeep",
        TurnStep::Draw => "draw step",
        TurnStep::PrecombatMain => "precombat main phase",
        TurnStep::BeginningOfCombat => "combat",
        TurnStep::DeclareAttackers => "declare attackers step",
        TurnStep::DeclareBlockers => "declare blockers step",
        TurnStep::FirstStrikeDamage => "first-strike damage step",
        TurnStep::CombatDamage => "combat damage step",
        TurnStep::EndOfCombat => "end of combat step",
        TurnStep::PostcombatMain => "postcombat main phase",
        TurnStep::End => "end step",
        TurnStep::Cleanup => "cleanup step",
    }
}

/// A step as an instruction names it.
pub(super) fn step_noun(step: TurnStep) -> &'static str {
    match step {
        TurnStep::Upkeep => "upkeep step",
        TurnStep::BeginningOfCombat => "beginning of combat step",
        TurnStep::Untap
        | TurnStep::Draw
        | TurnStep::PrecombatMain
        | TurnStep::DeclareAttackers
        | TurnStep::DeclareBlockers
        | TurnStep::FirstStrikeDamage
        | TurnStep::CombatDamage
        | TurnStep::EndOfCombat
        | TurnStep::PostcombatMain
        | TurnStep::End
        | TurnStep::Cleanup => step_name(step),
    }
}
