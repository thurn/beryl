//! Placeholder glyphs and icons for headless interface checks.

use kit_base::atlas::artwork::LayerCell;
use kit_base::atlas::{AssetReady, GlyphCell, GlyphKey};
use kit_ui::art::icons::ALL;
use kit_ui::widgets::icon_shadows;

/// Page 0 holds the real atlas's shapes of light.
pub const STUB_PAGE: u16 = 1;

/// A cell for `key`, as wide as half its size and as tall as its size, standing on the
/// baseline: a glyph's usual proportions, so that its record lies on its line.
pub fn stub_glyph(key: GlyphKey) -> GlyphCell {
    let height = key.size.max(1);
    GlyphCell {
        key,
        rect: [0, 0, (height / 2).max(1), height],
        bearing: [0, -i16::try_from(height).unwrap_or(i16::MAX)],
    }
}

/// Placeholder cells with the sizes the worker delivers, including any caller-supplied layers.
pub fn stub_icons(scale: f32, layers: impl IntoIterator<Item = LayerCell>) -> AssetReady {
    let sharp = ALL.iter().map(|icon| {
        let side = icon.side(scale);
        icon.cell([0, 0, side, side], 0)
    });
    let shadows = icon_shadows::cells().map(|(id, code)| {
        let artwork = &ALL[usize::from(id.0)];
        let (chain, stage) = icon_shadows::decode(code).expect("a listed shadow");
        let side =
            icon_shadows::stage_side(artwork.size, icon_shadows::shadows(chain), stage, scale);
        artwork.cell([0, 0, side, side], code)
    });
    let layers = layers.into_iter().map(|layer| {
        let [width, height] = layer.texels(scale);
        layer.cell([0, 0, width, height])
    });
    AssetReady::Icons {
        page: STUB_PAGE,
        cells: sharp.chain(shadows).chain(layers).collect(),
    }
}
