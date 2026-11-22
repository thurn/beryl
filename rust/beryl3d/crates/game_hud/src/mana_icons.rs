//! The icons that card and prompt text name with mana notation.

use std::borrow::Cow;

use kit_base::tokens::symbol;
use kit_base::typography::fonts::Font;
use kit_text::text::InlineIcon;

/// Looks up a mana symbol by name. Unknown symbols show their own letters.
#[must_use]
pub fn icon(name: &str) -> InlineIcon<'_> {
    let (background, ink) = colors(name);
    InlineIcon {
        glyph: code(name).map_or_else(
            || (Cow::Borrowed(name), None),
            |glyph| (Cow::Owned(glyph.to_string()), Some(Font::Symbols)),
        ),
        background,
        ink,
    }
}

/// The Mana font's character for `symbol`, if the font supplies a glyph.
#[must_use]
fn code(symbol: &str) -> Option<char> {
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
fn colors(symbol: &str) -> ([u8; 4], [u8; 4]) {
    match symbol {
        "W" => (symbol::W, symbol::INK),
        "U" => (symbol::U, symbol::U_INK),
        "B" => (symbol::B, symbol::INK),
        "R" => (symbol::R, symbol::INK),
        "G" => (symbol::G, symbol::INK),
        "C" => (symbol::C, symbol::INK),
        _ => (symbol::GENERIC, symbol::INK),
    }
}

#[cfg(test)]
mod tests {
    use super::code;

    #[test]
    fn numbers_up_to_fifteen_have_glyphs() {
        assert_eq!(code("0"), Some('\u{e605}'));
        assert_eq!(code("15"), Some('\u{e614}'));
        assert_eq!(code("16"), None);
        assert_eq!(code("W/U"), None);
    }
}
