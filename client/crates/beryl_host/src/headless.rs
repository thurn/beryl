//! The headless host (docs/beryl-3d.md, "Hosts"): it plays a recording through the client with
//! the test driver's clock and draws the frame at any moment with the real renderer into an
//! offscreen target. It has no window and no timer, so a picture of a moment is the same on
//! every run. `client png` and `client film` are built on it.
//!
//! It waits for the shared asset service before each picture and uses only scans already in
//! the cache, so a picture never depends on downloads arriving mid-replay.

use std::path::Path;

use app_client::driver::{DriverError, Page};
use beryl_art::Origins;
use beryl_render::{Limits, Renderer, Target};
use game_probe::driver::Moment;
use kit_base::data::lighting::Lighting;
use kit_frame::frame::Frame;

use crate::assets::{Assets, Scans};

/// An address where nothing listens, so that a scan the cache lacks fails to download at once.
const NOWHERE: Origins<'static> = Origins {
    cards: "http://127.0.0.1:9",
    backs: "http://127.0.0.1:9",
};

/// A scene that can supply frames and receive the assets they need.
pub trait HeadlessPage: Sized {
    #[must_use]
    fn with_host_atlas(self) -> Self;
    fn scale(&self) -> f32;
    /// The lights, shadow map and bloom the renderer draws the scene with.
    fn lighting(&self) -> Lighting;
    fn frame(&mut self) -> &Frame;
    fn prepare_assets(&mut self, assets: &mut Assets, renderer: &mut Renderer);
}

impl HeadlessPage for Page {
    fn with_host_atlas(self) -> Self {
        self.with_host_atlas()
    }

    fn scale(&self) -> f32 {
        self.scale()
    }

    fn lighting(&self) -> Lighting {
        self.data().lighting
    }

    fn frame(&mut self) -> &Frame {
        self.frame()
    }

    fn prepare_assets(&mut self, assets: &mut Assets, renderer: &mut Renderer) {
        let (client, atlas) = self.host_assets();
        assets.settle(client, renderer, |ready| atlas.push(ready));
    }
}

/// A scene played in one window, and the renderer that draws it.
pub struct Headless<P = Page> {
    page: P,
    renderer: Renderer,
    target: Target,
    assets: Assets,
}

impl<P: HeadlessPage> Headless<P> {
    /// Draws `page`, whose window is `width` by `height` logical points, at the page's scale
    /// ([`Page::with_scale`]), with its card faces read from `scans`. Its glyphs and icons are
    /// rasterized at that scale, as the desktop host's are, so a picture at scale 2 shows what a
    /// Retina display does.
    ///
    /// # Errors
    ///
    /// When there is no GPU adapter or device.
    pub fn new(page: P, width: f32, height: f32, scans: &Scans) -> Result<Self, String> {
        let page = page.with_host_atlas();
        let scale = page.scale();
        let renderer = Renderer::headless(Limits::Default, page.lighting())?;
        let pixels = |points: f32| (points * scale).round().max(1.0) as u32;
        let target = Target::offscreen(&renderer, pixels(width), pixels(height));
        let assets = Assets::start(scans, NOWHERE, || {});
        Ok(Self {
            page,
            renderer,
            target,
            assets,
        })
    }

    fn prepare(&mut self) {
        self.page.frame();
        self.page
            .prepare_assets(&mut self.assets, &mut self.renderer);
    }

    /// The scene being played, to read or to act on.
    pub fn page(&mut self) -> &mut P {
        &mut self.page
    }

    /// The size of every picture, in physical pixels.
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        (self.target.width, self.target.height)
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

impl Headless<Page> {
    /// Moves the recording to `moment`.
    ///
    /// # Errors
    ///
    /// When the recording has no such moment.
    pub fn seek(&mut self, moment: &Moment) -> Result<(), DriverError> {
        self.page.seek(moment)
    }
}
