//! Glyphs for a host that draws a page's text: the test driver itself never rasterizes, so it
//! hands the glyphs the client wants to the host and gives the client the pages that come back.

use super::Page;
use crate::input::{AssetReady, GlyphKey, Input};

impl Page {
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
}
