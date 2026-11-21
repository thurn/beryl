//! The selected layout profile and its regions in one window.

use kit_base::data::profiles::{ProfileData, Profiles, Region, Row};
use kit_base::geometry::{Rect, Window};
use kit_scene::{SizeClass, Slot};

use crate::table::{Camera, Layout, RowArea, TraySpace};

mod desktop;
pub mod phone;
pub(crate) mod web_fit;
pub use desktop::REFERENCE;
pub(crate) use desktop::tray_tuning;
pub use phone::{Band, TableBands};
pub use web_fit::{FittedPiles, FittedRow, FittedRows};

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

    /// Fits the table's containers for this window.
    #[must_use]
    pub fn layout(self) -> Layout {
        if let Some(banded) = self.data.table.banded {
            phone::layout(self.data, self.window, banded)
        } else {
            desktop::layout(self.data, self.window)
        }
    }

    /// Fits just the prompt tray, without projecting the table's cards and piles.
    #[must_use]
    pub fn tray(self) -> TraySpace {
        match (self.data.table.banded, self.bands()) {
            (Some(banded), Some(bands)) => phone::tray_in(
                self.window,
                &bands,
                banded,
                self.data.table.bands.expect("validated banded table"),
            ),
            _ => REFERENCE.tray,
        }
    }

    /// The regions of the table, if the profile supplies window bands.
    #[must_use]
    pub fn bands(self) -> Option<TableBands> {
        let table = &self.data.table;
        let sizes = table.bands?;
        let area = phone::battlefield_with(self.window, sizes);
        let camera = Camera::fit_inside(table.camera, self.window, area);
        Some(phone::bands_with(
            &camera,
            sizes,
            table.creatures.y,
            table.lands.y,
        ))
    }

    /// Resolves a named slot in logical points. An unknown name has no slot.
    #[must_use]
    pub fn slot(self, name: &str) -> Option<Slot> {
        let spec = *self.data.slots.get(name)?;
        let safe = Slot::safe_area(self.window);
        let region = match spec.region {
            Region::SafeArea => safe,
            region => {
                let bands = self.bands().expect("validated slot region");
                let band = match region {
                    Region::OpponentStrip => bands.opponent_strip,
                    Region::YourStrip => bands.your_strip,
                    Region::Middle => bands.middle,
                    Region::PromptArea => Band {
                        top: bands.middle.top,
                        height: bands.your_strip.bottom() - bands.middle.top,
                    },
                    Region::LogArea => Band {
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

fn row(data: Row, centred: bool) -> RowArea {
    RowArea {
        y: data.y,
        left: data.left,
        right: data.right,
        centre: centred.then_some(f32::midpoint(data.left, data.right)),
    }
}
