//! Glyphs and full-size faces for a host that draws a page: the test driver itself never
//! rasterizes or prepares a face, so it hands what the client wants to the host and gives the
//! client what comes back.

use super::Page;
use crate::input::{AssetReady, FaceId, GlyphKey, Input};

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
