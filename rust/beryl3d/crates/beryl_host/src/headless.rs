//! The headless host (docs/beryl-3d.md, "Hosts"): it plays a recording through the client with
//! the test driver's clock and draws the frame at any moment with the real renderer into an
//! offscreen target. It has no window and no timer, so a picture of a moment is the same on
//! every run. `b3d png` and `b3d film` are built on it.
//!
//! Before its first picture it has `beryl_assets`' workers prepare every face of the recording's
//! match and uploads them all, as the app does behind its loading screen (docs/beryl-3d.md,
//! "Preparation"), so a card shows its scan when the scan cache has it and Beryl's drawn card
//! frame when not. It never downloads a scan, so a picture depends only on what the cache holds.
//!
//! Text is drawn from glyphs that `beryl_assets`' glyph worker rasterizes on its own thread:
//! the printable ASCII characters of every type style before the first picture, and before each
//! picture whatever else the client's text asks for. The host waits for the worker to finish
//! before it draws, so a picture never lacks a glyph that a later one would have.

use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};

use beryl_art::{ArtCache, Origins};
use beryl_assets::glyphs::{GlyphWorker, prepare_latin};
use beryl_assets::light;
use beryl_assets::{Config, Format, Prepared, Preparer, default_workers};
use beryl_render::{Limits, Renderer, Target};

use crate::assets::Scans;
use beryl_scene::driver::{DriverError, Moment, Page};
use beryl_scene::faces::FaceList;
use beryl_scene::frame::Frame;

/// An address where nothing listens, so that a scan the cache lacks fails to download at once.
const NOWHERE: Origins<'static> = Origins {
    cards: "http://127.0.0.1:9",
    backs: "http://127.0.0.1:9",
};

/// A recording played for one seat in one window, and the renderer that draws it.
pub struct Headless {
    page: Page,
    renderer: Renderer,
    target: Target,
    glyphs: GlyphWorker,
    /// The glyph pages the worker has finished.
    pages: Receiver<Prepared>,
}

impl Headless {
    /// Draws `page`, whose window is `width` by `height` logical points, at the page's scale
    /// ([`Page::with_scale`]), with its card faces read from `scans`. Its glyphs and icons are
    /// rasterized at that scale, as the desktop host's are, so a picture at scale 2 shows what a
    /// Retina display does.
    ///
    /// # Errors
    ///
    /// When there is no GPU adapter or device.
    pub fn new(page: Page, width: f32, height: f32, scans: &Scans) -> Result<Self, String> {
        let scale = page.scale();
        let mut renderer = Renderer::headless(Limits::Default)?;
        renderer.upload_page(&light::prepared());
        load_faces(&mut renderer, page.faces(), scans);
        let pixels = |points: f32| (points * scale).round().max(1.0) as u32;
        let target = Target::offscreen(&renderer, pixels(width), pixels(height));
        let (out, pages) = mpsc::channel();
        let glyphs = GlyphWorker::spawn(out, || {}, crate::threads::asset_thread);
        glyphs.prepare(prepare_latin(scale));
        glyphs.icons(scale);
        let mut head = Self {
            page,
            renderer,
            target,
            glyphs,
            pages,
        };
        head.text();
        Ok(head)
    }

    /// Has the glyph worker rasterize what the present moment's text needs that the client has
    /// no page for, waits for it, uploads every page it finished (the first time, the icons'
    /// too) and gives the client their glyphs and icons. Laying out text never waits for its glyphs, so this changes what is drawn but
    /// not where.
    fn text(&mut self) {
        self.page.frame();
        self.glyphs.request(self.page.wanted_glyphs());
        self.glyphs.flush();
        while let Ok(prepared) = self.pages.try_recv() {
            if self.renderer.upload_page(&prepared) {
                self.page.give_glyphs(prepared.ready);
            }
        }
    }

    /// The recording being played, to read or to act on.
    pub fn page(&mut self) -> &mut Page {
        &mut self.page
    }

    /// The size of every picture, in physical pixels.
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        (self.target.width, self.target.height)
    }

    /// Moves the recording to `moment`.
    ///
    /// # Errors
    ///
    /// When the recording has no such moment.
    pub fn seek(&mut self, moment: &Moment) -> Result<(), DriverError> {
        self.page.seek(moment)
    }

    /// The frame at the present moment, as the client builds it.
    pub fn frame(&mut self) -> &Frame {
        self.text();
        self.page.frame()
    }

    /// Draws the present frame and returns its pixels, RGBA bytes row by row from the top.
    pub fn pixels(&mut self) -> Vec<u8> {
        self.text();
        let frame = self.page.frame();
        self.renderer.render(frame, &mut self.target);
        self.renderer.read(&self.target)
    }

    /// Draws the present frame with only the records of the elements `keep` accepts, by their
    /// index in the frame's `resolved`, and returns its pixels as `pixels` does. The page must
    /// be looking (`Page::set_looking`), so that the frame knows which element each record
    /// belongs to (`Frame::owners`); otherwise nothing is drawn.
    pub fn pixels_keeping(&mut self, keep: impl Fn(&Frame, u32) -> bool) -> Vec<u8> {
        self.text();
        let mut frame = self.page.frame().clone();
        let wanted: Vec<bool> = (0..frame.resolved.len())
            .map(|at| keep(&frame, at as u32))
            .collect();
        frame.keep_only(|owner| wanted.get(owner as usize).copied().unwrap_or(false));
        self.renderer.render(&frame, &mut self.target);
        self.renderer.read(&self.target)
    }

    /// Draws the present frame into a PNG file at `path`.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn png(&mut self, path: &Path) -> Result<(), String> {
        self.text();
        let frame = self.page.frame();
        self.renderer.render(frame, &mut self.target);
        self.renderer.write_png(&self.target, path)
    }
}

/// Makes `renderer`'s face arrays for the faces of `list` and uploads every face, as the asset
/// workers read it from the compressed cache, compress it from its scan or draw its frame.
/// This thread only waits and uploads: the workers do every file read, decode and compression.
fn load_faces(renderer: &mut Renderer, list: &FaceList, scans: &Scans) {
    let format = Format::choose(&renderer.formats());
    let names: Vec<&str> = list
        .faces()
        .iter()
        .map(|face| face.names.first().map_or("", String::as_str))
        .collect();
    renderer.set_faces(&names, format);
    let mut preparer = Preparer::new(Config {
        art: Arc::new(ArtCache::new(scans.art.clone(), NOWHERE)),
        faces_dir: scans.faces.clone(),
        workers: default_workers(),
        wake: Arc::new(|| {}),
        start: crate::threads::asset_thread,
    });
    let results = preparer.prepare_match(list, &[format]);
    // Dropping the preparer waits for its workers to finish every face queued, after which the
    // channel holds them all and then ends.
    drop(preparer);
    for prepared in results {
        renderer.upload_layer(&prepared);
    }
}
