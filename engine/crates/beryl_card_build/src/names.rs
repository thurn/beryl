//! How a card's name and a set's code are spelled as Rust modules and constants. Python's
//! `set_groups.card_module` and `set_groups.set_module` are the same rules, by which the tools
//! place and find a card's file.

/// Rust's strict and reserved keywords, which name no module unescaped.
const KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate",
    "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "gen", "if", "impl",
    "in", "let", "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref",
    "return", "self", "static", "struct", "super", "trait", "true", "try", "type", "typeof",
    "union", "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
];

/// A letter with its accent dropped, for the accented letters card names have.
fn unaccented(c: char) -> Option<char> {
    Some(match c {
        'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ā' => 'a',
        'é' | 'è' | 'ê' | 'ë' | 'ē' => 'e',
        'í' | 'ì' | 'î' | 'ï' | 'ī' => 'i',
        'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ō' => 'o',
        'ú' | 'ù' | 'û' | 'ü' | 'ū' => 'u',
        'ñ' => 'n',
        'ç' => 'c',
        'Á' | 'À' | 'Â' | 'Ä' => 'A',
        'É' | 'È' | 'Ê' => 'E',
        'Í' => 'I',
        'Ó' | 'Ö' => 'O',
        'Ú' | 'Ü' => 'U',
        _ => return None,
    })
}

/// `name` as a module: itself where it is an identifier and no keyword, else `<prefix>_<name>`.
fn module(name: String, prefix: &str) -> String {
    let starts = name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_');
    if starts && !KEYWORDS.contains(&name.as_str()) {
        name
    } else {
        format!("{prefix}_{name}")
    }
}

/// A set's module: its code, prefixed where the code names no module (`set_10e`, `set_mod`).
pub fn set_module(code: &str) -> String {
    module(code.to_string(), "set")
}

/// A card's module, from its name: its ASCII letters and digits in lowercase words joined by
/// underscores. Accents and apostrophes drop, and any other character ends a word
/// (`lim_duls_vault`, `fire_ice`); a name that would start with a digit or be a keyword takes
/// a prefix (`card_2_mace`, `card_static`).
///
/// # Errors
///
/// When the name has a letter outside ASCII that [`unaccented`] does not know, or no letter or
/// digit at all.
pub fn card_module(name: &str) -> Result<String, String> {
    let mut out = String::new();
    for c in name.chars() {
        let c = unaccented(c).unwrap_or(c);
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if c.is_alphabetic() {
            return Err(format!(
                "{name:?} has the letter {c:?}, which beryl_card_build's names.rs cannot spell \
                 in ASCII: add it to `unaccented`"
            ));
        } else if !matches!(c, '\'' | '’') && !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_end_matches('_').to_string();
    if out.is_empty() {
        return Err(format!("{name:?} has no ASCII letter or digit"));
    }
    Ok(module(out, "card"))
}

/// A card's constant in a `names` module: its module in capitals, so
/// "Jace, Vryn's Prodigy" is `JACE_VRYNS_PRODIGY`.
///
/// # Errors
///
/// As [`card_module`].
pub fn constant(name: &str) -> Result<String, String> {
    card_module(name).map(|module| module.to_ascii_uppercase())
}

/// A card's name as an associated constant of the generated `Card` and `Printed` types: its
/// module's words, each capitalized, so "Jace, Vryn's Prodigy" is `JaceVrynsProdigy`.
///
/// # Errors
///
/// As [`card_module`].
pub fn camel(name: &str) -> Result<String, String> {
    card_module(name).map(|module| {
        module
            .split('_')
            .map(|word| {
                let mut chars = word.chars();
                chars.next().map_or_else(String::new, |first| {
                    first.to_ascii_uppercase().to_string() + chars.as_str()
                })
            })
            .collect()
    })
}

/// The [`constant`] a [`camel`] spelling most likely came from: a word starts at each capital
/// letter and at each change between letters and digits, so `Card2Mace` is `CARD_2_MACE`. The
/// build looks the result up in the card store and checks it spells back to `camel`.
pub fn constant_of_camel(camel: &str) -> String {
    let mut out = String::new();
    let mut previous: Option<char> = None;
    for c in camel.chars() {
        if let Some(p) = previous {
            let starts = c.is_ascii_uppercase()
                || (c.is_ascii_digit() && !p.is_ascii_digit())
                || (!c.is_ascii_digit() && p.is_ascii_digit());
            if starts {
                out.push('_');
            }
        }
        out.push(c.to_ascii_uppercase());
        previous = Some(c);
    }
    out
}
