//! Side-by-side or stacked arrangements with a vertical cascade and a curved row.

use glam::Vec2;
use kit_base::geometry::Rect;

use super::{FanShape, Grid, ScreenFit, ScreenLayout, gaps, inset};

/// The geometry of two adjacent piles. Counts and capacities follow display order.
#[derive(Clone, Copy)]
pub struct CascadeFan {
    pub cascade_step: f32,
    pub cascade_turns: Vec2,
    /// The rise of the base behind the cascade, as a fraction of item height.
    pub cascade_rise: f32,
    pub separation: f32,
    pub fan: FanShape,
    pub fan_min_step: f32,
    pub preferred_step: f32,
    pub heading: f32,
}

pub struct Paired {
    /// Items in the cascade run from deepest to frontmost.
    pub cascade: ScreenLayout,
    pub fan: ScreenLayout,
    /// The cascade's reserved positions, deepest first, including empty slots.
    pub slots: Vec<Rect>,
    /// The raised base behind the first slot.
    pub base: Rect,
    /// The outer drop regions, in cascade and fan order.
    pub bounds: [Rect; 2],
}

impl CascadeFan {
    #[must_use]
    pub fn fit(
        self,
        fit: ScreenFit,
        band: Rect,
        second: Option<Rect>,
        counts: [usize; 2],
        capacity: [usize; 2],
    ) -> Paired {
        let (cascade, fan, slots) = self.arrange(fit, band, second, counts, capacity);
        let first = slots[0];
        let base = Rect {
            y: first.y - self.cascade_rise * first.height,
            ..first
        };
        let last = slots[slots.len() - 1];
        let cascade_band = clamp_band(
            base.y - self.heading,
            bottom(last).min(bottom(cascade.viewport)) + fit.gap,
            band,
        );
        let fan_band = clamp_band(
            fan.viewport.y - self.heading - fit.gap,
            bottom(fan.viewport) + fit.gap,
            second.unwrap_or(band),
        );
        let drop_region = |(y, end), span: Rect| Rect {
            y,
            height: end - y,
            ..span
        };
        let bounds = if second.is_some() {
            [drop_region(cascade_band, band), drop_region(fan_band, band)]
        } else {
            let split = (base.x + base.width + fan.viewport.x) / 2.0;
            let both = (
                cascade_band.0.min(fan_band.0),
                cascade_band.1.max(fan_band.1),
            );
            [
                drop_region(
                    both,
                    Rect {
                        width: split - band.x,
                        ..band
                    },
                ),
                drop_region(
                    both,
                    Rect {
                        x: split,
                        width: band.x + band.width - split,
                        ..band
                    },
                ),
            ]
        };
        Paired {
            cascade,
            fan,
            slots,
            base,
            bounds,
        }
    }

    fn arrange(
        self,
        fit: ScreenFit,
        band: Rect,
        second: Option<Rect>,
        counts: [usize; 2],
        [depth, room]: [usize; 2],
    ) -> (ScreenLayout, ScreenLayout, Vec<Rect>) {
        let gap = fit.gap.min(band.width / 4.0).min(band.height / 4.0);
        let area = inset(band, gap);
        let hand = second.map_or(area, |second| {
            let mut second = inset(second, gap.min(second.height / 4.0));
            let heading = self.heading.min(second.height / 2.0);
            second.y += heading;
            second.height -= heading;
            second
        });
        let width_limit = if second.is_some() {
            area.width.min(hand.width)
        } else {
            area.width / (2.0 + self.separation)
        };
        let width_limit = width_limit
            .min(area.height.min(hand.height) / fit.aspect)
            .max(0.0);
        let fit = ScreenFit {
            most: fit.most.min(width_limit),
            ..fit
        };
        // The cascade has a base even when no item is reserved above it.
        let depth = depth.max(1);
        for width in fit.widths().take_while(|&width| width >= fit.press) {
            let item = fit.size(width);
            let deck_step = (self.cascade_step * item.y)
                .max(self.preferred_step)
                .min(Grid::pitch(area.height, item.y, depth, 0.0))
                .max(fit.press);
            let deck_height = item.y + deck_step * gaps(depth);
            let available = if second.is_some() {
                hand.width
            } else {
                hand.width - width * (1.0 + self.separation)
            };
            let fan_step = (self.fan.step * width)
                .max(self.preferred_step)
                .max(fit.press)
                .min(Grid::pitch(available, width, room, 0.0).max(fit.press));
            let reserved = width + fan_step * gaps(room);
            if deck_height > area.height
                || reserved > available
                || (room > 1 && fan_step < self.fan_min_step * width)
            {
                continue;
            }
            let whole = width * (1.0 + self.separation) + reserved;
            let x = if second.is_some() {
                area.x + (area.width - width) / 2.0
            } else {
                area.x + (area.width - whole) / 2.0
            };
            let deck = Rect {
                x,
                y: area.y + (area.height - deck_height) / 2.0,
                width,
                height: deck_height,
            };
            let fan_region = if second.is_some() {
                hand
            } else {
                Rect {
                    x: x + width * (1.0 + self.separation),
                    width: reserved,
                    ..hand
                }
            };
            let (mut cascade, slots) = self.column(counts[0], depth, item, deck_step, deck, false);
            // Include the rotation of the deepest visible cards in the fit. Use the
            // band for this test so the turns may extend into the reserved padding.
            cascade.viewport = band;
            let mut fan = self.fan.row(counts[1], item, fan_step, fan_region);
            let largest_fan = self.fan.row(room, item, fan_step, fan_region);
            if cascade.fits(fit.press) && fan.fits(fit.press) && largest_fan.fits(fit.press) {
                let height = item.y + self.fan.drop * width;
                fan.viewport.y += (fan.viewport.height - height) / 2.0;
                fan.viewport.height = height;
                return (cascade, fan, slots);
            }
        }
        self.scrolling(fit, area, hand, second.is_some(), counts, depth)
    }

    fn scrolling(
        self,
        fit: ScreenFit,
        area: Rect,
        hand: Rect,
        stacked: bool,
        [count, hand_count]: [usize; 2],
        depth: usize,
    ) -> (ScreenLayout, ScreenLayout, Vec<Rect>) {
        let width = fit.least.min(fit.most);
        let item = fit.size(width);
        let x = if stacked {
            area.x + (area.width - width) / 2.0
        } else {
            area.x
        };
        let step = (self.cascade_step * item.y)
            .max(self.preferred_step)
            .max(fit.press)
            .min(item.y);
        let height = (item.y + step * gaps(depth)).min(area.height);
        let deck = Rect {
            x,
            y: area.y + (area.height - height) / 2.0,
            width,
            height,
        };
        let (cascade, slots) = self.column(count, depth, item, step, deck, true);
        let hand = if stacked {
            hand
        } else {
            let start = width * (1.0 + self.separation);
            Rect {
                x: hand.x + start,
                width: (hand.width - start).max(0.0),
                ..hand
            }
        };
        let fan = ScreenFit {
            most: width,
            least: width,
            ..fit
        }
        .scrolling_rows(hand_count, hand);
        (cascade, fan, slots)
    }

    fn column(
        self,
        count: usize,
        depth: usize,
        item: Vec2,
        step: f32,
        region: Rect,
        straight: bool,
    ) -> (ScreenLayout, Vec<Rect>) {
        let grid = Grid::new(1, item, Vec2::new(0.0, step - item.y));
        let mut cascade = ScreenLayout::grid(grid, count, region, false);
        if !straight {
            for (index, item) in cascade.items.iter_mut().enumerate() {
                if index + 1 < count {
                    item.turn = self.cascade_turns[index % 2];
                }
            }
        }
        (cascade, grid.boxes(depth, region, false).collect())
    }
}

fn bottom(rect: Rect) -> f32 {
    rect.y + rect.height
}

fn clamp_band(top: f32, end: f32, region: Rect) -> (f32, f32) {
    let bottom = bottom(region);
    (top.clamp(region.y, bottom), end.clamp(region.y, bottom))
}
