//! Glyphs and full-size faces for a host that draws a page: the test driver itself never
//! rasterizes or prepares a face, so it hands what the client wants to the host and gives the
//! client what comes back. A page with no such host, as in every interface test, has a stub
//! atlas instead: the driver acts as a host whose glyph worker delivers every glyph the client
//! asks for before the frame that asked is read, and every icon at the one size the atlas holds
//! it (`icons::atlas_size`), with the blurred copies its drop shadows need. It rasterizes nothing: each cell has a made-up place on the page,
//! enough for the client to draw a record for it. The "drawn" check then reports what no host
//! could deliver: text drawn with a glyph it never asked for, and an icon drawn at a size the
//! atlas does not hold.

use super::Page;
use crate::client;
use client::Input;
use game_hud::art::orb;
use kit_base::atlas::{AssetReady, FaceId, GlyphCell, GlyphKey};
use kit_probe::driver::atlas::{STUB_PAGE, stub_glyph, stub_icons};
use kit_ui::input::Input as DeviceInput;

pub(super) fn stub_client_icons(scale: f32) -> AssetReady {
    stub_icons(scale, orb::atlas_cells())
}

impl Page {
    /// The same page at its start, with the atlas and face layers empty until a host that
    /// draws supplies its uploads.
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
        self.give(Input::Device(DeviceInput::Assets(ready)));
    }

    /// The full-size faces the client asked for since the last call, each once.
    pub fn wanted_faces(&mut self) -> Vec<FaceId> {
        self.client.wanted_faces().collect()
    }

    /// Gives the client an uploaded face's layer.
    pub fn give_face(&mut self, ready: AssetReady) {
        self.give(Input::Device(DeviceInput::Assets(ready)));
    }

    /// Gives the drawing host the replay's client and the atlas deliveries it retains across
    /// backward seeks. Record each atlas delivery only after the renderer accepts its page.
    pub fn host_assets(&mut self) -> (&mut client::Client, &mut Vec<AssetReady>) {
        (&mut self.client, &mut self.glyphs)
    }
}
