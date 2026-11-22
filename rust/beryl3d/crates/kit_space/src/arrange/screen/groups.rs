//! Fits groups with space reserved for their headings and for items moving between them.

use glam::Vec2;
use kit_base::geometry::Rect;

use super::{Grid, ScreenFit, ScreenLayout, gaps};

/// The current count and the preferred reservation for a group.
#[derive(Clone, Copy)]
pub struct ScreenGroup {
    pub count: usize,
    pub room: usize,
}

/// A group's outer bounds and the items and scroll viewport inside them.
pub struct ScreenGroupLayout {
    pub bounds: Rect,
    pub layout: ScreenLayout,
}

impl ScreenFit {
    /// Arrange groups in input order. Two groups can use separate vertical bands
    /// with the same horizontal span. Otherwise all groups share the first band.
    #[must_use]
    pub fn groups(
        self,
        groups: &[ScreenGroup],
        band: Rect,
        second: Option<Rect>,
        heading: f32,
    ) -> Vec<ScreenGroupLayout> {
        match (groups, second) {
            ([], _) => Vec::new(),
            ([first, last], Some(second)) => self.stacked([*first, *last], band, second, heading),
            _ => self.side_by_side(groups, band, heading),
        }
    }

    fn side_by_side(
        self,
        groups: &[ScreenGroup],
        band: Rect,
        heading: f32,
    ) -> Vec<ScreenGroupLayout> {
        let columns = groups.len() as f32;
        let between = groups
            .iter()
            .map(|group| group.room.saturating_sub(1))
            .sum::<usize>() as f32;
        let edges = 3.0 * columns - 1.0;
        let gap = self
            .gap
            .min(band.width / (edges + columns))
            .min(band.height / 3.0)
            .max(0.0);
        let heading = heading.min((band.height - 2.0 * gap).max(0.0));
        let fit = Self { gap, ..self };
        let (width, pitch) = fit.parallel(
            Vec2::new(band.width - edges * gap, band.height - heading - 2.0 * gap),
            columns,
            between,
        );
        let item = fit.size(width);
        let height = item.y + heading + 2.0 * gap;
        let whole = columns * width + between * pitch + edges * gap;
        let y = band.y + ((band.height - height) / 2.0).max(0.0);
        let mut x = band.x + (band.width - whole) / 2.0;
        groups
            .iter()
            .map(|group| {
                let inner = width + gaps(group.room) * pitch;
                let bounds = Rect {
                    x,
                    y,
                    width: inner + 2.0 * gap,
                    height,
                };
                x += bounds.width + gap;
                fit.reserve(*group, bounds, inner, item, pitch, heading)
            })
            .collect()
    }

    fn stacked(
        self,
        groups: [ScreenGroup; 2],
        band: Rect,
        second: Rect,
        heading: f32,
    ) -> Vec<ScreenGroupLayout> {
        let height_limit = band.height.min(second.height).max(0.0);
        let gap = self
            .gap
            .min(band.width / 3.0)
            .min(height_limit / 3.0)
            .max(0.0);
        let heading = heading.min(height_limit - 2.0 * gap);
        let inner = band.width - 2.0 * gap;
        let room = groups[0].room.max(groups[1].room);
        let width = (inner - self.press * gaps(room))
            .clamp(self.least, self.most)
            .min(
                inner
                    .min((height_limit - heading - 2.0 * gap) / self.aspect)
                    .max(0.0),
            );
        let fit = Self { gap, ..self };
        let item = fit.size(width);
        let height = item.y + heading + 2.0 * gap;
        groups
            .into_iter()
            .zip([band, second])
            .map(|(group, band)| {
                let bounds = Rect {
                    y: band.y + ((band.height - height) / 2.0).max(0.0),
                    height,
                    ..band
                };
                let pitch = Grid::pitch(inner, width, group.room.max(2), gap);
                fit.reserve(group, bounds, inner, item, pitch, heading)
            })
            .collect()
    }

    /// Compress the reserved strips at the minimum preferred item width. Actual
    /// items wrap if that compressed pitch cannot expose their press targets.
    fn parallel(self, usable: Vec2, columns: f32, between: f32) -> (f32, f32) {
        let limit = (usable.x / columns).min(usable.y / self.aspect).max(0.0);
        let fit = Self {
            most: self.most.min(limit),
            ..self
        };
        for width in fit.widths() {
            let pitch = width + self.gap;
            if columns * width + between * pitch <= usable.x {
                return (width, pitch);
            }
        }
        let width = fit.least.min(fit.most);
        let pitch = (width + self.gap).min((usable.x - columns * width) / between.max(1.0));
        (width, pitch.max(0.0))
    }

    fn reserve(
        self,
        group: ScreenGroup,
        bounds: Rect,
        inner: f32,
        item: Vec2,
        pitch: f32,
        heading: f32,
    ) -> ScreenGroupLayout {
        let viewport = Rect {
            x: bounds.x + self.gap,
            y: bounds.y + heading + self.gap,
            width: inner,
            height: item.y,
        };
        let grid = Grid::reserved_row(
            group.count,
            viewport.width,
            item,
            pitch,
            self.gap,
            self.press.min(item.x),
        );
        ScreenGroupLayout {
            bounds,
            layout: ScreenLayout::grid(grid, group.count, viewport, false),
        }
    }
}
