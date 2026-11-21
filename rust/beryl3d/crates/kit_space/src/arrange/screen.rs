//! Fits screen-facing items while keeping each item reachable in the viewport.

use glam::Vec2;
use kit_base::geometry::Rect;

use super::Grid;
use super::press::{PressArea, Quad, Rectangle};

/// One screen-facing rectangle, with a clockwise turn in degrees.
#[derive(Clone, Copy, Debug)]
pub struct ScreenItem {
    pub rect: Rect,
    pub turn: f32,
}

impl ScreenItem {
    fn quad(self) -> Quad {
        let (sin, cos) = self.turn.to_radians().sin_cos();
        let half = Vec2::new(self.rect.width, self.rect.height) / 2.0;
        Quad::new(
            Rectangle {
                centre: Vec2::new(self.rect.x, self.rect.y) + half,
                half,
                axis: Vec2::new(cos, sin),
            }
            .corners(),
        )
    }
}

/// Item positions before scrolling, and the fixed window through which they are seen.
pub struct ScreenLayout {
    pub items: Vec<ScreenItem>,
    pub viewport: Rect,
    pub scroll: f32,
    pub rows: bool,
}

impl ScreenLayout {
    #[must_use]
    pub fn grid(grid: Grid, count: usize, viewport: Rect, centred: bool) -> Self {
        Self {
            items: grid
                .boxes(count, viewport, centred)
                .map(|rect| ScreenItem { rect, turn: 0.0 })
                .collect(),
            viewport,
            scroll: (grid.size(count).y - viewport.height).max(0.0),
            rows: count > grid.columns,
        }
    }

    fn fits(&self, press: f32) -> bool {
        let screen = ScreenItem {
            rect: self.viewport,
            turn: 0.0,
        }
        .quad();
        let quads: Vec<_> = self.items.iter().map(|item| item.quad()).collect();
        // A card's exposed square must be inside the viewport and clear of every
        // card drawn over it, including neighbours whose rotation crosses its edge.
        quads.iter().enumerate().all(|(index, item)| {
            let press = PressArea::new(item, &screen, press);
            press
                .witness(
                    quads[index + 1..]
                        .iter()
                        .filter(|blocker| blocker.bounds.overlaps(item.bounds)),
                )
                .is_some()
        })
    }
}

/// Sizing and spacing for equal screen-facing rectangles.
#[derive(Clone, Copy)]
pub struct ScreenFit {
    pub aspect: f32,
    pub most: f32,
    pub least: f32,
    pub gap: f32,
    pub press: f32,
}

impl ScreenFit {
    pub fn widths(self) -> impl Iterator<Item = f32> {
        let least = self.least.min(self.most);
        let mut width = self.most;
        std::iter::from_fn(move || {
            let tried = width;
            width = if tried > least {
                (tried - 8.0).max(least)
            } else {
                -1.0
            };
            (tried >= 0.0).then_some(tried)
        })
    }

    #[must_use]
    pub fn size(self, width: f32) -> Vec2 {
        Vec2::new(width, width * self.aspect)
    }

    #[must_use]
    pub fn fitting_width(self, region: Rect) -> f32 {
        region.width.min(region.height / self.aspect).max(0.0)
    }

    #[must_use]
    pub fn rows(self, count: usize, region: Rect) -> ScreenLayout {
        ScreenLayout::grid(self.row_grid(count, region), count, region, true)
    }

    fn row_grid(self, count: usize, region: Rect) -> Grid {
        Grid::fit(
            count,
            Vec2::new(region.width, region.height),
            self.aspect,
            self.gap,
            self.press,
            self.widths(),
        )
    }

    fn scrolling_rows(self, count: usize, region: Rect) -> ScreenLayout {
        let mut grid = self.row_grid(count, region);
        grid.step.y = grid.item.y + self.gap;
        ScreenLayout::grid(grid, count, region, true)
    }

    /// Uniform widths for parallel reserved strips. Only the preferred room is
    /// compressed. The actual items can wrap without changing the reservation.
    #[must_use]
    pub fn parallel(self, usable: Vec2, strips: usize, between: usize) -> (f32, f32) {
        let strips = strips.max(1) as f32;
        let between = between as f32;
        let limit = (usable.x / strips).min(usable.y / self.aspect).max(0.0);
        let fit = Self {
            most: self.most.min(limit),
            ..self
        };
        for width in fit.widths() {
            let pitch = width + self.gap;
            if strips * width + between * pitch <= usable.x {
                return (width, pitch);
            }
        }
        let width = fit.least.min(fit.most);
        let pitch = (width + self.gap).min((usable.x - strips * width) / between.max(1.0));
        (width, pitch.max(0.0))
    }

    /// Shared width for strips with the same horizontal span in separate bands.
    #[must_use]
    pub fn stacked_width(self, usable: Vec2, room: usize) -> f32 {
        (usable.x - self.press * gaps(room))
            .clamp(self.least, self.most)
            .min(usable.x.min(usable.y / self.aspect).max(0.0))
    }

    /// A reserved strip wraps at the press target. The original width stays
    /// fixed while items move in and out of the reservation.
    #[must_use]
    pub fn reserved(self, count: usize, region: Rect, width: f32, pitch: f32) -> ScreenLayout {
        let grid = Grid::reserved_row(
            count,
            region.width,
            self.size(width),
            pitch,
            self.gap,
            self.press.min(width),
        );
        ScreenLayout::grid(grid, count, region, false)
    }

    #[must_use]
    pub fn reserved_pitch(self, region: Rect, width: f32, capacity: usize) -> f32 {
        Grid::pitch(region.width, width, capacity.max(2), self.gap)
    }

    /// Fits a fan when every rotated item has a press square. A flat grid wraps
    /// and scrolls when that fan would hide a card.
    #[must_use]
    pub fn fan(self, count: usize, region: Rect, shape: FanShape) -> ScreenLayout {
        let gaps = gaps(count);
        let inner = inset(region, self.gap);
        let width = (inner.height / (self.aspect + shape.drop))
            .min(inner.width / (1.0 + shape.step * gaps))
            .min(inner.width - self.press * gaps)
            .max(self.least)
            .min(self.fitting_width(inner));
        let pitch = (shape.step * width)
            .min(Grid::pitch(inner.width, width, count, 0.0))
            .max(self.press);
        if count <= 1 || width + pitch * gaps <= inner.width {
            let fan = shape.row(count, self.size(width), pitch, region);
            if fan.fits(self.press) {
                return fan;
            }
        }
        self.scrolling_rows(count, region)
    }
}

/// A curved row, with its preferred step and end drop as fractions of item width.
#[derive(Clone, Copy)]
pub struct FanShape {
    pub step: f32,
    pub drop: f32,
    pub turn: f32,
}

impl FanShape {
    fn row(self, count: usize, size: Vec2, pitch: f32, region: Rect) -> ScreenLayout {
        let drop = self.drop * size.x;
        let grid = Grid::new(count, size, Vec2::new(pitch - size.x, 0.0));
        let mut band = region;
        band.y -= drop / 2.0;
        let mut fan = ScreenLayout::grid(grid, count, band, true);
        fan.viewport = region;
        for (index, item) in fan.items.iter_mut().enumerate() {
            let along = if count > 1 {
                2.0 * index as f32 / gaps(count) - 1.0
            } else {
                0.0
            };
            item.rect.y += drop * along * along;
            item.turn = self.turn * along;
        }
        fan
    }
}

fn gaps(count: usize) -> f32 {
    count.saturating_sub(1) as f32
}

#[must_use]
pub fn inset(region: Rect, gap: f32) -> Rect {
    let x = gap.min(region.width / 2.0).max(0.0);
    let y = gap.min(region.height / 2.0).max(0.0);
    Rect {
        x: region.x + x,
        y: region.y + y,
        width: (region.width - 2.0 * x).max(0.0),
        height: (region.height - 2.0 * y).max(0.0),
    }
}

mod pair;
pub use pair::{CascadeFan, Paired};
