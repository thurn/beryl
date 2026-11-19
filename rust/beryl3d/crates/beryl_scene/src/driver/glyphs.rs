//! Glyphs and full-size faces for a host that draws a page: the test driver itself never
//! rasterizes or prepares a face, so it hands what the client wants to the host and gives the
//! client what comes back. A page with no such host, as in every interface test, has a stub
//! atlas instead: the driver acts as a host whose glyph worker delivers every glyph the client
//! asks for before the frame that asked is read, and every icon at the one size the atlas holds
//! it (`icons::atlas_size`). It rasterizes nothing: each cell has a made-up place on the page,
//! enough for the client to draw a record for it. The "drawn" check then reports what no host
//! could deliver: text drawn with a glyph it never asked for, and an icon drawn at a size the
//! atlas does not hold.

use super::Page;
use crate::art::orb;
use crate::input::{AssetReady, FaceId, GlyphCell, GlyphKey, IconCell, Input};
use crate::widgets::IconId;
use crate::widgets::icons::{ORB, atlas_size};

/// The atlas page the stub atlas puts every glyph and icon on. Page 0 holds the shapes of light,
/// as in a real host's atlas.
const STUB_PAGE: u16 = 1;

impl Page {
    /// The same page at its start, with no stub atlas: a host that draws, such as the headless
    /// one, rasterizes the glyphs the client wants (`wanted_glyphs`) and the icons, and gives
    /// them (`give_glyphs`). Until it does, the client's text and icons draw nothing.
    #[must_use]
    pub fn with_host_atlas(mut self) -> Self {
        if self.stub_atlas {
            self.stub_atlas = false;
            // The stub's glyph pages, which a restart would give again.
            self.glyphs.clear();
            self.restart();
        }
        self
    }

    /// Gives the client a stub cell for every glyph it has asked for since the last call.
    pub(super) fn deliver_stub_glyphs(&mut self) {
        let glyphs: Vec<GlyphCell> = self.client.wanted_glyphs().map(stub_glyph).collect();
        if !glyphs.is_empty() {
            self.give_glyphs(AssetReady::Glyphs {
                page: STUB_PAGE,
                glyphs,
            });
            self.render();
        }
    }

    /// The glyphs the client's laid-out text needs and no page it was given holds, each once:
    /// what a host hands its glyph rasterizer (`Client::wanted_glyphs`).
    pub fn wanted_glyphs(&mut self) -> Vec<GlyphKey> {
        self.client.wanted_glyphs().collect()
    }

    /// Gives the client a glyph page's `AssetReady::Glyphs`, once the host has uploaded the
    /// page, and keeps it to give again whenever the client is made anew.
    pub fn give_glyphs(&mut self, ready: AssetReady) {
        self.glyphs.push(ready.clone());
        self.give(Input::Assets(ready));
    }

    /// The faces the client asked for at full size since the last call, each once: what a host
    /// hands its asset workers (`Client::wanted_faces`).
    pub fn wanted_faces(&mut self) -> Vec<FaceId> {
        self.client.wanted_faces().collect()
    }

    /// Gives the client a face's `AssetReady::Face`, once the host has uploaded it.
    pub fn give_face(&mut self, ready: AssetReady) {
        self.give(Input::Assets(ready));
    }
}

/// A cell for `key`, as wide as half its size and as tall as its size, standing on the
/// baseline: a glyph's usual proportions, so that its record lies on its line.
fn stub_glyph(key: GlyphKey) -> GlyphCell {
    let height = key.size.max(1);
    GlyphCell {
        key,
        rect: [0, 0, (height / 2).max(1), height],
        bearing: [0, -i16::try_from(height).unwrap_or(i16::MAX)],
    }
}

/// Every icon at its atlas size, and each layer of each life orb at its width for each of the
/// orbs' scales, as a host's glyph worker delivers them before the first frame.
pub(super) fn stub_icons() -> AssetReady {
    let mut cells: Vec<IconCell> = (0..ORB)
        .map(|id| {
            let icon = IconId(id);
            let size = atlas_size(icon);
            IconCell {
                icon,
                size,
                rect: [0, 0, size, size],
            }
        })
        .collect();
    for units in orb::SCALES {
        for side in orb::SIDES {
            for (at, layer) in orb::LAYERS.iter().enumerate() {
                let [_, _, w, h] = layer.views[orb::seat(side)];
                let width = orb::cell_width(w, units);
                let height = orb::cell_width(h, units);
                cells.push(IconCell {
                    icon: orb::icon(side, at),
                    size: width,
                    rect: [0, 0, width.max(1), height.max(1)],
                });
            }
        }
    }
    AssetReady::Icons {
        page: STUB_PAGE,
        cells,
    }
}
