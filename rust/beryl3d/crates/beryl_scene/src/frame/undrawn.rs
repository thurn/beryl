//! What a frame could not draw in full for lack of an atlas cell.

/// An element that a frame could not draw in full, because the glyph and icon tables held no
/// atlas cell for part of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Undrawn {
    /// The index in `Frame::resolved` of the element.
    pub element: u32,
    pub missing: Missing,
}

/// What an element lacked an atlas cell for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Missing {
    /// This many glyphs of its text had no cell at their own key (font, weight, glyph and size
    /// in physical pixels). Each was drawn from another size of itself, scaled, or not at all.
    Glyphs(u32),
    /// Its icon had no cell at the size, in logical points, it is drawn at, so it drew nothing.
    Icon { icon: u16, size: u16 },
}
