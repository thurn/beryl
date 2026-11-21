//! The headless host (docs/beryl-3d.md, "Hosts"): it plays a recording through the client with
//! the test driver's clock and draws the frame at any moment with the real renderer into an
//! offscreen target. It has no window and no timer, so a picture of a moment is the same on
//! every run. `b3d png` and `b3d film` are built on it.
//!
//! It waits for the shared asset service before each picture and uses only scans already in
//! the cache, so a picture never depends on downloads arriving mid-replay.

use std::path::Path;

use app_client::driver::{DriverError, Page};
use beryl_art::Origins;
use beryl_render::{Limits, Renderer, Target};
use game_probe::driver::Moment;
use kit_frame::frame::Frame;

use crate::assets::{Assets, Scans};

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
    assets: Assets,
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
        let page = page.with_host_atlas();
        let scale = page.scale();
        let mut renderer = Renderer::headless(Limits::Default)?;
        let pixels = |points: f32| (points * scale).round().max(1.0) as u32;
        let target = Target::offscreen(&renderer, pixels(width), pixels(height));
        let assets = Assets::start(&mut renderer, scans, NOWHERE, || {});
        Ok(Self {
            page,
            renderer,
            target,
            assets,
        })
    }

    fn prepare(&mut self) {
        self.page.frame();
        let (client, atlas) = self.page.host_assets();
        self.assets
            .settle(client, &mut self.renderer, |delivery| atlas.push(delivery));
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
        self.prepare();
        self.page.frame()
    }

    /// Draws the present frame and returns its pixels, RGBA bytes row by row from the top.
    pub fn pixels(&mut self) -> Vec<u8> {
        self.prepare();
        let frame = self.page.frame();
        self.renderer.render(frame, &mut self.target);
        self.renderer.read(&self.target)
    }

    /// Draws the present frame with only the records of the elements `keep` accepts, by their
    /// index in the frame's `resolved`, and returns its pixels as `pixels` does. The page must
    /// be looking (`Page::set_looking`), so that the frame knows which element each record
    /// belongs to (`Frame::owners`); otherwise nothing is drawn.
    pub fn pixels_keeping(&mut self, keep: impl Fn(&Frame, u32) -> bool) -> Vec<u8> {
        self.prepare();
        let mut frame = self.page.frame().clone();
        let wanted: Vec<bool> = (0..frame.resolved.len())
            .map(|at| keep(&frame, at as u32))
            .collect();
        frame.keep_only(|owner| wanted.get(owner as usize).copied().unwrap_or(false));
        self.renderer.render(&frame, &mut self.target);
        self.renderer.read(&self.target)
    }

    /// Draws the present frame with only the records `keep` accepts, by their index in the
    /// frame's `draws`, and returns its pixels as `pixels` does.
    pub fn pixels_keeping_records(&mut self, keep: impl Fn(&Frame, usize) -> bool) -> Vec<u8> {
        self.prepare();
        let mut frame = self.page.frame().clone();
        let wanted: Vec<bool> = (0..frame.draws.len()).map(|at| keep(&frame, at)).collect();
        frame.keep_records(|at| wanted[at]);
        self.renderer.render(&frame, &mut self.target);
        self.renderer.read(&self.target)
    }

    /// Draws the present frame into a PNG file at `path`.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn png(&mut self, path: &Path) -> Result<(), String> {
        self.prepare();
        let frame = self.page.frame();
        self.renderer.render(frame, &mut self.target);
        self.renderer.write_png(&self.target, path)
    }
}
