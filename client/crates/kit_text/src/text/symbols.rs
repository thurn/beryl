//! Named inline icons in text. The caller chooses the font, glyph and colors.

use std::ops::Range;

use std::borrow::Cow;

use kit_base::typography::fonts::Font;

/// The character parley lays out an inline box at.
pub const OBJECT: char = '\u{fffc}';

/// An inline icon's glyph size relative to its one-em box.
pub const GLYPH_SIZE: f32 = 0.8;

/// How far an inline box hangs below the baseline, in ems.
pub const DROP: f32 = 0.1;

/// The byte ranges of the names delimited by braces, including the braces.
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

/// The glyph and colors of an inline icon.
pub struct InlineIcon<'a> {
    /// Glyph text and its font. With no font, it uses the font of the surrounding text.
    pub glyph: (Cow<'a, str>, Option<Font>),
    pub background: [u8; 4],
    pub ink: [u8; 4],
}

/// Resolves an inline name as an icon. A text shaper without a resolver reads braces literally.
pub type IconResolver = fn(&str) -> InlineIcon<'_>;

#[cfg(test)]
mod tests {
    use super::tokens;

    #[test]
    fn names_are_found_between_nonempty_braces() {
        assert_eq!(tokens("Go {left}{up}"), vec![3..9, 9..13]);
        assert_eq!(tokens("{} and {up}"), vec![7..11]);
        assert!(tokens("open {up").is_empty());
    }
}
