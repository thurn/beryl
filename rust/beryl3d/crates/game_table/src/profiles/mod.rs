//! The selected layout profile and its regions in one window.

use kit_base::data::profiles::{ProfileData, Profiles, Region};
use kit_base::geometry::{Rect, Window};
use kit_scene::{SizeClass, Slot};

use crate::table::{Camera, phone};

/// The layout selected for a window. A feature uses its slots and size class to fit its content.
#[derive(Clone, Copy, Debug)]
pub struct Profile<'a> {
    data: &'a ProfileData,
    window: Window,
}

impl<'a> Profile<'a> {
    /// Selects the profile using logical points, independent of pixel density.
    #[must_use]
    pub fn for_window(profiles: &'a Profiles, window: Window) -> Self {
        Self {
            data: profiles.for_size(window.size),
            window,
        }
    }

    #[must_use]
    pub fn data(self) -> &'a ProfileData {
        self.data
    }

    #[must_use]
    pub fn size_class(self) -> SizeClass {
        self.data.size_class
    }

    /// Resolves a named slot in logical points. An unknown name has no slot.
    #[must_use]
    pub fn slot(self, name: &str) -> Option<Slot> {
        let spec = *self.data.slots.get(name)?;
        let safe = Slot::safe_area(self.window);
        let region = match spec.region {
            Region::SafeArea => safe,
            region => {
                let table = &self.data.table;
                let sizes = table.bands.expect("validated slot region");
                let area = phone::battlefield_with(self.window, sizes);
                let camera = Camera::fit_inside(table.camera, self.window, area);
                let bands = phone::bands_with(&camera, sizes, table.creatures.y, table.lands.y);
                let band = match region {
                    Region::OpponentStrip => bands.opponent_strip,
                    Region::YourStrip => bands.your_strip,
                    Region::Middle => bands.middle,
                    Region::PromptArea => phone::Band {
                        top: bands.middle.top,
                        height: bands.your_strip.bottom() - bands.middle.top,
                    },
                    Region::LogArea => phone::Band {
                        top: bands.opponent_strip.bottom(),
                        height: bands.hand.top - bands.opponent_strip.bottom(),
                    },
                    Region::SafeArea => unreachable!("handled above"),
                };
                Rect {
                    x: bands.left,
                    y: band.top,
                    width: bands.right - bands.left,
                    height: band.height,
                }
            }
        };
        Some(Slot::fit(spec, region, safe))
    }
}
