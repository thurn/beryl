//! Integration tests for text shaping.

mod serif;
mod shaping;
mod truncate;

fn icon(name: &str) -> kit_text::text::InlineIcon<'_> {
    kit_text::text::InlineIcon {
        glyph: (std::borrow::Cow::Borrowed(name), None),
        background: [if name == "B" { 200 } else { 100 }, 100, 100, 255],
        ink: [0, 0, 0, 255],
    }
}
