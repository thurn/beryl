//! Mana symbols written in a text as `{...}`, such as `{1}{U}` in "Pay {1}{U}" (docs/beryl-3d.md,
//! "Text": mana symbols are inline boxes). The web interface draws them with `RulesText` and
//! `ManaSymbol` in `ui/src/components/Mana.tsx`, styled by `.mana-symbol` in
//! `ui/src/design/components/mana-symbols.css`: a disc the size of the line's em in the symbol's
//! color, with the symbol's glyph from the Mana font in dark ink on it.

use std::ops::Range;

use kit_base::tokens::mana;

/// The character parley lays out an inline box at.
pub const OBJECT: char = '\u{fffc}';

/// A symbol's glyph is this many of the line's em: `.mana-symbol`'s font size is 0.82 of its
/// text's, and its disc is 1.25 of that, so the glyph is about 0.8 of the disc.
pub const GLYPH_SIZE: f32 = 0.8;

/// How far a symbol hangs below the baseline, in ems: `.mana-symbol`'s `vertical-align` of
/// -0.12 of its own 0.82 em.
pub const DROP: f32 = 0.1;

/// The byte ranges of the symbols in `text`, each with its braces: a `{`, one or more bytes
/// that are not `}`, and a `}`, as `RulesText` matches them with `/\{[^}]+\}/`.
#[must_use]
pub fn tokens(text: &str) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(open) = text[from..].find('{').map(|at| from + at) {
        match text[open + 1..].find('}') {
            Some(0) => from = open + 1,
            Some(length) => {
                let end = open + 1 + length + 1;
                found.push(open..end);
                from = end;
            }
            None => break,
        }
    }
    found
}

/// The Mana font's character for `symbol` (the text between the braces), from `CODES` in
/// `ui/src/components/Mana.tsx`, or `None` for a symbol the font has no glyph for.
#[must_use]
pub fn code(symbol: &str) -> Option<char> {
    let point = match symbol {
        "W" => 0xe600,
        "U" => 0xe601,
        "B" => 0xe602,
        "R" => 0xe603,
        "G" => 0xe604,
        "C" => 0xe904,
        "X" => 0xe615,
        "S" => 0xe619,
        "T" => 0xe61a,
        "Q" => 0xe61b,
        "E" => 0xe907,
        number => 0xe605 + number.parse::<u32>().ok().filter(|&n| n < 16)?,
    };
    char::from_u32(point)
}

/// The disc's color and the glyph's ink for `symbol`: one of the five colors or colorless, or
/// the generic color for a number or any other symbol, as `ManaSymbol` picks its class.
#[must_use]
pub fn colors(symbol: &str) -> ([u8; 4], [u8; 4]) {
    match symbol {
        "W" => (mana::W, mana::INK),
        "U" => (mana::U, mana::U_INK),
        "B" => (mana::B, mana::INK),
        "R" => (mana::R, mana::INK),
        "G" => (mana::G, mana::INK),
        "C" => (mana::C, mana::INK),
        _ => (mana::GENERIC, mana::INK),
    }
}

#[cfg(test)]
mod tests {
    use super::{code, tokens};

    #[test]
    fn tokens_are_found_as_rules_text_finds_them() {
        assert_eq!(tokens("Pay {1}{U}"), vec![4..7, 7..10]);
        assert_eq!(tokens("{} and {T}"), vec![7..10]);
        assert_eq!(tokens("open {U"), Vec::<std::ops::Range<usize>>::new());
    }

    #[test]
    fn numbers_up_to_fifteen_have_glyphs() {
        assert_eq!(code("0"), Some('\u{e605}'));
        assert_eq!(code("15"), Some('\u{e614}'));
        assert_eq!(code("16"), None);
        assert_eq!(code("W/U"), None);
    }
}
