//! The selected layout profile and its regions in one window.

use beryl_protocol::protocol::Side;
use glam::Vec2;
use kit_base::data::Card;
use kit_base::data::profiles::{ProfileData, Profiles, Region, Row};
use kit_base::geometry::{Rect, Window};
use kit_scene::{SizeClass, Slot};
use kit_ui::widgets::screen::ScreenColumn;

use crate::table::{Camera, Layout, RegionFit, RowArea, ScreenProjection, TraySpace};

mod desktop;
mod phone;
pub mod wide_compact;
pub use kit_base::geometry::Band;
pub use phone::TableBands;

/// The layout selected for a window. A feature uses its slots and size class to fit its content.
#[derive(Clone, Copy, Debug)]
pub struct Profile<'a> {
    data: &'a ProfileData,
    fallback: &'a ProfileData,
    window: Window,
    region_fit: RegionFit,
    screen_projection: ScreenProjection,
}

impl<'a> Profile<'a> {
    /// Selects the profile using logical points, independent of pixel density.
    #[must_use]
    pub fn for_window(profiles: &'a Profiles, window: Window) -> Self {
        let region_fit = RegionFit::default();
        Self {
            data: profiles.for_size(window.size),
            fallback: profiles.fallback(),
            window,
            region_fit,
            screen_projection: region_fit.screen(window),
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

    /// Supplies a policy for fitting the table's regions.
    #[must_use]
    pub fn with_region_fit(self, fit: RegionFit) -> Self {
        self.with_fitted_projection(fit, fit.screen(self.window))
    }

    /// Reuses the screen projection already fitted for this window.
    pub(crate) fn with_fitted_projection(
        mut self,
        fit: RegionFit,
        screen_projection: ScreenProjection,
    ) -> Self {
        self.region_fit = fit;
        self.screen_projection = screen_projection;
        self
    }

    /// The projection used by screen elements laid out in reference coordinates.
    #[must_use]
    pub fn screen_projection(self) -> ScreenProjection {
        self.screen_projection
    }

    /// Fits the table's containers for this window.
    #[must_use]
    pub fn layout(self) -> Layout {
        let table = &self.data.table;
        let mut layout = if let Some(banded) = table.banded {
            phone::layout(self.data, self.window, banded)
        } else {
            let anchored = table.anchored.as_ref().expect("validated anchored table");
            desktop::reference(self.data, anchored)
        };
        layout.tray = self.fit_tray(layout.tray);
        self.region_fit.apply(self.data, self.window, &mut layout);
        layout
    }

    /// Builds an anchored table without fitting it to the window or the web stage.
    #[must_use]
    pub fn reference_layout(self) -> Option<Layout> {
        let anchored = self.data.table.anchored.as_ref()?;
        Some(desktop::reference(self.data, anchored))
    }

    /// Fits just the prompt tray, without projecting the table's cards and piles.
    #[must_use]
    pub fn tray(self) -> TraySpace {
        let table = &self.data.table;
        if let Some(anchored) = &table.anchored {
            return desktop::tray(
                anchored.tray,
                table.tray_tuning,
                table.camera.card,
                self.data.checks.target_points,
            );
        }
        phone::tray_in(
            self.window,
            &self.bands().expect("validated banded table"),
            table.banded.expect("validated banded table"),
            table.bands.expect("validated banded table"),
            (
                table.tray_tuning,
                table.camera.card,
                self.data.checks.target_points,
            ),
        )
    }

    /// Fits the order row using its own tray, then the available anchored tray.
    #[must_use]
    pub fn order_tray(self) -> TraySpace {
        let table = &self.data.table;
        let fallback = &self.fallback.table;
        let tray = table
            .order_tray
            .or(table.anchored.as_ref().map(|anchored| anchored.tray))
            .map(|tray| (tray, table.tray_tuning))
            .or_else(|| {
                fallback
                    .anchored
                    .as_ref()
                    .map(|anchored| (anchored.tray, fallback.tray_tuning))
            });
        let Some((tray, tuning)) = tray else {
            return self.tray();
        };
        desktop::tray(
            tray,
            tuning,
            table.camera.card,
            self.data.checks.target_points,
        )
    }

    /// The regions of the table, if the profile supplies window bands.
    #[must_use]
    pub fn bands(self) -> Option<TableBands> {
        let table = &self.data.table;
        let sizes = table.bands?;
        let area = phone::battlefield_with(self.window, sizes, table.camera.card);
        let camera = Camera::fit_inside(table.camera, self.window, area);
        Some(phone::bands_with(
            &camera,
            sizes,
            table.creatures.y,
            table.lands.y,
        ))
    }

    // Keep the tray's stage units while projecting its bounds into the fitted table. The
    // tray solver otherwise treats any letterboxing as part of the playable surface.
    fn fit_tray(self, mut tray: TraySpace) -> TraySpace {
        let Some((stage, _)) = self.fitted_stage() else {
            return tray;
        };
        let window = self.window.size;
        let fit = stage.height / window.height;
        let per_point = tray.stage_height / window.height;
        tray.y = tray.y * fit + stage.y * per_point;
        tray.height *= fit;
        tray.most *= fit;
        tray.least *= fit;
        tray.gap *= fit;
        tray.heading *= fit;
        if let crate::table::Across::Middle {
            half_most,
            stack_side,
        } = &mut tray.across
        {
            *half_most *= fit;
            *stack_side = *stack_side * fit + stage.x * per_point;
        }
        tray
    }

    /// The optional fitted stage from which desktop layout takes its size and margins.
    #[must_use]
    pub fn fitted_stage(self) -> Option<(Rect, f32)> {
        self.data.table.anchored.as_ref()?;
        let projection = self.screen_projection();
        let size = projection.stage_size?;
        Some((
            Rect {
                x: projection.origin.x,
                y: projection.origin.y,
                width: size.width * projection.scale,
                height: size.height * projection.scale,
            },
            projection.scale,
        ))
    }

    #[must_use]
    pub fn window(self) -> Window {
        self.window
    }

    /// The card's printed size and stacking (`card.json`), which the profile's camera carries.
    #[must_use]
    pub fn card(self) -> Card {
        self.data.table.camera.card
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
                phone::strip_with(self.window, side, (bands, self.data.table.camera.card)).middle(),
            );
        }
        let index = usize::from(side == Side::Opponent);
        let anchored = self
            .data
            .table
            .anchored
            .as_ref()
            .expect("validated anchored table");
        self.anchor_point(anchored.players[index].life)
    }

    /// The top left of the player's mana pool in logical points.
    #[must_use]
    pub fn pool_position(self, side: Side) -> Vec2 {
        let index = usize::from(side == Side::Opponent);
        self.data.table.anchored.as_ref().map_or_else(
            || {
                let offset = self
                    .data
                    .table
                    .banded
                    .map_or(Vec2::ZERO, |data| data.pool_offset);
                self.player_centre(side) + offset
            },
            |anchored| self.anchor_point(anchored.players[index].pool),
        )
    }

    fn anchor_point(self, ndc: Vec2) -> Vec2 {
        self.window.size.screen(ndc)
    }

    /// The inner edges of the shared library and graveyard on a banded table, in logical points
    /// from the window's left: the library's right edge and the graveyard's left edge. When both
    /// players share their piles, the phone lays the two at the ends of the middle band, so HUD
    /// furniture and prompt controls placed in that band must stand between these edges. `None`
    /// for a profile with no banded table.
    #[must_use]
    pub fn shared_pile_edges(self) -> Option<(f32, f32)> {
        let piles = self.data.table.banded?.shared_piles;
        let inset = self.window.safe_area;
        let half = piles.width / 2.0;
        Some((
            inset.left + piles.left + half,
            self.window.size.width - inset.right - piles.right - half,
        ))
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
                let strip =
                    phone::strip_with(self.window, side, (sizes, self.data.table.camera.card));
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
                    Region::PlayArea => Band {
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
