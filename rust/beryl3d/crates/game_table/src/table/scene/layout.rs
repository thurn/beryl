//! Building the table and refitting its layout when the window changes.

use std::sync::Arc;

use beryl_protocol::protocol::PlayerView;
use kit_base::data::Data;
use kit_frame::frame::OBJECTS;

use super::{Fights, Objects, Placed, RowFits, Scroll, Shown, Table, TrayMotion, scroll};
use crate::profiles::Profile;
use crate::table::{RegionFit, Window};

impl Table {
    #[must_use]
    pub fn new(data: Arc<Data>, window: Window) -> Self {
        Self::with_region_fit(data, window, RegionFit::default())
    }

    /// Creates a table whose regions can be fitted by the caller's policy.
    #[must_use]
    pub fn with_region_fit(data: Arc<Data>, window: Window, fit: RegionFit) -> Self {
        let window = data.profiles.resolve_window(window);
        let profile = Profile::for_window(&data.profiles, window).with_region_fit(fit);
        let layout = profile.layout();
        let screen_projection = profile.screen_projection();
        let camera = layout.fit(window);
        Self {
            data,
            layout,
            screen_projection,
            row_fits: RowFits::new(&layout, &camera),
            camera,
            scroll: Scroll::default(),
            slide: scroll::Slide::default(),
            objects: Objects::default(),
            placed: Placed::default(),
            revealed: Vec::new(),
            fights: Fights::with_capacity(OBJECTS),
            held: None,
            tray: TrayMotion::new(),
            shown: Shown::default(),
            prompt_covers: false,
            hand_away: false,
        }
    }

    /// The selected profile before its table geometry is fitted.
    #[must_use]
    pub fn profile(&self) -> Profile<'_> {
        Profile::for_window(&self.data.profiles, self.camera.window)
            .with_fitted_projection(self.layout.region_fit, self.screen_projection)
    }

    /// Fits the camera to a window of a new shape, in the layout that shape takes, and
    /// recomputes every resting place.
    pub fn resize(&mut self, window: Window, view: Option<&PlayerView>) {
        let window = self.data.profiles.resolve_window(window);
        let profile = Profile::for_window(&self.data.profiles, window)
            .with_region_fit(self.layout.region_fit);
        self.screen_projection = profile.screen_projection();
        self.layout = profile.layout();
        if self.hand_away {
            // The stage fills the height of the window, so two normalized device units span
            // its full height in stage points.
            self.layout.hand.edge -=
                2.0 * self.layout.style.hand_away / self.layout.tray.stage_height;
        }
        self.camera = self.layout.fit(window);
        self.row_fits = RowFits::new(&self.layout, &self.camera);
        if let Some(view) = view {
            self.apply(view);
        }
    }
}
