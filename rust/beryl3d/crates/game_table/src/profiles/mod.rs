//! The selected layout profile and its regions in one window.

use beryl_protocol::protocol::Side;
use glam::Vec2;
use kit_base::data::profiles::{ProfileData, Profiles, Region, Row};
use kit_base::geometry::{Rect, Window};
use kit_scene::{SizeClass, Slot};
use kit_ui::widgets::screen::ScreenColumn;

use crate::table::{Camera, Layout, RowArea, TraySpace};

mod desktop;
pub mod phone;
pub(crate) mod web_fit;
pub use desktop::REFERENCE;
pub(crate) use desktop::tray_tuning;
pub use phone::{Band, TableBands};
pub use web_fit::{FittedPiles, FittedRow, FittedRows};

/// The logical points per unit of life-orb art, in atlas order.
pub const ORB_SCALES: [f32; 2] = [REFERENCE.play.life.orb_scale, phone::ORB_SCALE];

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

    #[must_use]
    pub fn window(self) -> Window {
        self.window
    }

    /// The scrolling column and action dock, when this profile supplies them.
    #[must_use]
    pub fn screen_column(self) -> Option<ScreenColumn> {
        Some(ScreenColumn {
            bounds: self.slot("screen.column")?.rect,
            metrics: self.data.screen?,
            target: self.data.checks.target_points,
        })
    }

    /// The initial centre of a player's life total, before the hand can raise it.
    #[must_use]
    pub fn player_centre(self, side: Side) -> Vec2 {
        let window = self.window.size;
        if let Some(bands) = self.data.table.bands {
            return Vec2::new(
                window.width / 2.0,
                phone::strip_with(self.window, side, bands).middle(),
            );
        }
        let index = usize::from(side == Side::Opponent);
        let ndc = REFERENCE.players[index].life;
        Vec2::new(
            f32::midpoint(ndc.x, 1.0) * window.width,
            f32::midpoint(1.0, -ndc.y) * window.height,
        )
    }

    /// Resolves a named slot in logical points. An unknown name has no slot.
    #[must_use]
    pub fn slot(self, name: &str) -> Option<Slot> {
        self.resolve_slot(name, || self.bands())
    }

    /// Resolves a group of slots against the same fitted table.
    #[must_use]
    pub fn slots<const N: usize>(self, names: [&str; N]) -> [Option<Slot>; N] {
        let bands = std::cell::OnceCell::new();
        names.map(|name| self.resolve_slot(name, || *bands.get_or_init(|| self.bands())))
    }

    fn resolve_slot(self, name: &str, bands: impl FnOnce() -> Option<TableBands>) -> Option<Slot> {
        let spec = *self.data.slots.get(name)?;
        let bounds = if spec.region == Region::Window {
            Rect {
                width: self.window.size.width,
                height: self.window.size.height,
                ..Rect::default()
            }
        } else {
            Slot::safe_area(self.window)
        };
        let region = match spec.region {
            Region::SafeArea | Region::Window => bounds,
            Region::PlayerYou | Region::PlayerOpponent => {
                let side = if spec.region == Region::PlayerYou {
                    Side::You
                } else {
                    Side::Opponent
                };
                centred(bounds, self.player_centre(side))
            }
            Region::YourStrip | Region::OpponentStrip => {
                let sizes = self.data.table.bands.expect("validated slot region");
                let side = if spec.region == Region::YourStrip {
                    Side::You
                } else {
                    Side::Opponent
                };
                let strip = phone::strip_with(self.window, side, sizes);
                let (left, right) = phone::column(self.window, sizes);
                Rect {
                    x: left,
                    y: strip.top,
                    width: right - left,
                    height: strip.height,
                }
            }
            region => {
                let bands = bands().expect("validated slot region");
                let band = match region {
                    Region::Middle => bands.middle,
                    Region::PromptArea => Band {
                        top: bands.middle.top,
                        height: bands.your_strip.bottom() - bands.middle.top,
                    },
                    Region::LogArea => Band {
                        top: bands.opponent_strip.bottom(),
                        height: bands.hand.top - bands.opponent_strip.bottom(),
                    },
                    Region::AboveViewerRows => Band {
                        top: bands.opponent_strip.top,
                        height: bands.your_creatures.top - bands.opponent_strip.top,
                    },
                    Region::Battlefield => Band {
                        top: bands.opponent_back.top,
                        height: bands.your_strip.top - bands.opponent_back.top,
                    },
                    Region::Catalog => Band {
                        top: bands.opponent_back.top,
                        height: bands.middle.bottom() - bands.opponent_back.top,
                    },
                    _ => unreachable!("unbanded region handled above"),
                };
                Rect {
                    x: bands.left,
                    y: band.top,
                    width: bands.right - bands.left,
                    height: band.height,
                }
            }
        };
        Some(Slot::fit(spec, region, bounds))
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

// The largest box within the safe area whose centre remains at the original anchor.
fn centred(safe: Rect, centre: Vec2) -> Rect {
    let start = Vec2::new(safe.x, safe.y);
    let end = start + Vec2::new(safe.width, safe.height);
    let centre = centre.clamp(start, end);
    let half = (centre - start).min(end - centre);
    Rect {
        x: centre.x - half.x,
        y: centre.y - half.y,
        width: 2.0 * half.x,
        height: 2.0 * half.y,
    }
}
