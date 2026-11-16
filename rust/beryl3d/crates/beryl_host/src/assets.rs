//! Text for the hosts that draw frames as they come, the desktop and windowless hosts
//! (docs/beryl-3d.md, "Text" and "Threads"). The client lays text out without waiting for its
//! glyphs and lists the glyphs no atlas page holds (`Client::wanted_glyphs`). `Glyphs` hands
//! them to `beryl_assets`' glyph worker, which rasterizes them on its own thread, and uploads
//! each page the worker finishes before the next frame, telling the client where its glyphs
//! landed (`Input::Assets`). The worker wakes the host when a page is ready, so a still table
//! draws the new text at once.
//!
//! The headless host does the same, but waits for the worker before each picture
//! (`Headless`), so that a picture never lacks a glyph.

use std::sync::mpsc::{self, Receiver};

use beryl_assets::Prepared;
use beryl_assets::glyphs::{GlyphWorker, prepare_latin};
use beryl_render::Renderer;
use beryl_scene::Client;
use beryl_scene::input::Input;

/// The glyph worker of a host, and the pages it has finished.
pub(crate) struct Glyphs {
    worker: GlyphWorker,
    pages: Receiver<Prepared>,
    /// The physical pixels to a point that the Latin glyphs and the icons were asked for at.
    scale: f32,
}

impl Glyphs {
    /// Starts the worker and asks it for the Latin glyphs and every icon at `scale` physical
    /// pixels to a point, before the first frame needs them. The worker calls `wake` after it
    /// sends each page.
    pub(crate) fn start(scale: f32, wake: impl Fn() + Send + 'static) -> Self {
        let (out, pages) = mpsc::channel();
        let worker = GlyphWorker::spawn(out, wake, crate::threads::asset_thread);
        worker.request(prepare_latin(scale));
        worker.icons(scale);
        Self {
            worker,
            pages,
            scale,
        }
    }

    /// Asks again for the Latin glyphs and the icons when the display's scale has changed, as
    /// when the window moves to a display of another density.
    pub(crate) fn rescale(&mut self, scale: f32) {
        if (scale - self.scale).abs() > f32::EPSILON {
            self.scale = scale;
            self.worker.request(prepare_latin(scale));
            self.worker.icons(scale);
        }
    }

    /// Asks the worker for the glyphs the client's last frame lacked, and uploads every page it
    /// has finished, giving the client each page's glyphs or icons. It never waits.
    pub(crate) fn deliver(&mut self, client: &mut Client, renderer: &mut Renderer) {
        self.worker.request(client.wanted_glyphs());
        while let Ok(prepared) = self.pages.try_recv() {
            if renderer.upload_page(&prepared) {
                client.handle(Input::Assets(prepared.ready));
            }
        }
    }
}
