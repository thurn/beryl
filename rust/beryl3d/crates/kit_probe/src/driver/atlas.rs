//! Placeholder glyphs and icons for headless interface checks.

use kit_ui::input::{AssetReady, GlyphCell, GlyphKey, IconCell};
use kit_ui::orb;
use kit_ui::widgets::IconId;
use kit_ui::widgets::icon_shadows;
use kit_ui::widgets::icons::{ORB, atlas_size};

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

/// Every icon at its atlas size, the cells its drop shadows are drawn from at `scale`
/// (`icon_shadows::cells`) at the size a real one has (`icon_shadows::stage_side`), and each
/// layer of each life orb at its width for each of the orbs' scales, as a host's glyph worker
/// delivers them before the first frame.
pub fn stub_icons(scale: f32) -> AssetReady {
    let sharp = (0..ORB).map(|id| (IconId(id), 0));
    let mut cells: Vec<IconCell> = sharp
        .chain(icon_shadows::cells())
        .map(|(icon, shadow)| {
            let size = atlas_size(icon);
            let texels = icon_shadows::decode(shadow).map_or(size, |(chain, stage)| {
                icon_shadows::stage_side(size, icon_shadows::shadows(chain), stage, scale)
            });
            IconCell {
                icon,
                size,
                rect: [0, 0, texels, texels],
                shadow,
            }
        })
        .collect();
    for units in orb::SCALES {
        for side in orb::ORIENTATIONS {
            for (at, layer) in orb::LAYERS.iter().enumerate() {
                let [_, _, w, h] = layer.views[orb::seat(side)];
                let width = orb::cell_width(w, units);
                let height = orb::cell_width(h, units);
                cells.push(IconCell {
                    icon: orb::icon(side, at),
                    size: width,
                    rect: [0, 0, width.max(1), height.max(1)],
                    shadow: 0,
                });
            }
        }
    }
    AssetReady::Icons {
        page: STUB_PAGE,
        cells,
    }
}
