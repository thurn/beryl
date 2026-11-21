//! Identifiers and atlas coordinates shared by the client and the renderer.

/// An icon of the flat layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IconId(pub u16);

/// Where one rasterized icon lies in its atlas page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IconCell {
    pub icon: IconId,
    /// The size the icon is drawn at, in logical points: the side of its square box.
    pub size: u16,
    /// The icon's square cell in texels, `[x, y, width, height]`, rasterized at the display's
    /// scale, so `size` times the scale texels wide, or for a shadow's cell the size
    /// `widgets::icon_shadows::stage_side` gives.
    pub rect: [u16; 4],
    /// 0 for the icon itself, or which of the cells its drop shadows are drawn from this is
    /// (`widgets::icon_shadows::code`): one shadow of one chain of `icon_shadows::CHAINS`.
    pub shadow: u8,
}

/// Where one rasterized glyph lies in its atlas page, and where its picture sits against the
/// glyph's pen position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphCell {
    pub key: GlyphKey,
    /// The glyph's rectangle in texels, `[x, y, width, height]`.
    pub rect: [u16; 4],
    /// The rectangle's top left corner relative to the pen position on the baseline, in
    /// physical pixels, with y down: a glyph that rises above the baseline has a negative y.
    pub bearing: [i16; 2],
}

/// An index into the list of faces the client made when the match loaded (`faces::FaceList`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FaceId(pub u16);

/// Which texture array a face is baked for: board size (256 by 360) or full size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceSize {
    Board,
    Full,
}

/// One glyph of one bundled font at one weight and one size in physical pixels, sharp or
/// blurred for a drop shadow.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GlyphKey {
    /// The index in `text::fonts::ALL` of the font file the glyph is drawn from
    /// (`Font::face`).
    pub font: u8,
    /// The CSS weight the variable font is drawn at.
    pub weight: u16,
    pub glyph: u16,
    /// 0 for the glyph itself, or the standard deviation of the Gaussian its coverage is
    /// blurred with, in quarters of a physical pixel: the shape of a drop shadow
    /// (`Style::drop_shadow`). A blurred cell is larger than the glyph's by three deviations
    /// on every side, and its bearing says so. It comes before `size` so that one blur's sizes
    /// of a glyph sort together (`Glyphs::nearest`).
    pub blur: u8,
    pub size: u16,
}

/// The two visual tiers, which differ only in shading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Baseline,
    Enhanced,
}
