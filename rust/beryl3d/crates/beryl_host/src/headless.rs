//! The headless host (docs/beryl-3d.md, "Hosts"): it plays a recording through the client with
//! the test driver's clock and draws the frame at any moment with the real renderer into an
//! offscreen target. It has no window and no timer, so a picture of a moment is the same on
//! every run. `b3d png` and `b3d film` are built on it.

use std::path::Path;

use beryl_render::{Limits, Renderer, Target};
use beryl_scene::driver::{DriverError, Moment, Page};
use beryl_scene::frame::Frame;

/// A recording played for one seat in one window, and the renderer that draws it.
pub struct Headless {
    page: Page,
    renderer: Renderer,
    target: Target,
}

impl Headless {
    /// Draws `page`, whose window is `width` by `height` logical points, at `scale` physical
    /// pixels to a point.
    ///
    /// # Errors
    ///
    /// When there is no GPU adapter or device.
    pub fn new(page: Page, width: f32, height: f32, scale: f32) -> Result<Self, String> {
        let renderer = Renderer::headless(Limits::Default)?;
        let pixels = |points: f32| (points * scale).round().max(1.0) as u32;
        let target = Target::offscreen(&renderer, pixels(width), pixels(height));
        Ok(Self {
            page,
            renderer,
            target,
        })
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
        self.page.frame()
    }

    /// Draws the present frame and returns its pixels, RGBA bytes row by row from the top.
    pub fn pixels(&mut self) -> Vec<u8> {
        let frame = self.page.frame();
        self.renderer.render(frame, &mut self.target);
        self.renderer.read(&self.target)
    }

    /// Draws the present frame into a PNG file at `path`.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn png(&mut self, path: &Path) -> Result<(), String> {
        let frame = self.page.frame();
        self.renderer.render(frame, &mut self.target);
        self.renderer.write_png(&self.target, path)
    }
}
