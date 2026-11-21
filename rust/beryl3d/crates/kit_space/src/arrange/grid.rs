//! The Grid policy's rows, in a plane or in a screen element's logical points.

use glam::Vec2;
use kit_base::geometry::Rect;

/// A grid of equal boxes in screen coordinates. Callers supply the item order and
/// the available space. The same cursor places variable-size packs in a table grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub columns: usize,
    pub item: Vec2,
    pub step: Vec2,
}

impl Grid {
    /// A grid with a gap between its boxes. A negative gap overlaps adjacent boxes.
    #[must_use]
    pub fn new(columns: usize, item: Vec2, gap: Vec2) -> Self {
        Self {
            columns: columns.max(1),
            item,
            step: item + gap,
        }
    }

    /// The greatest number of columns of this size that fit across a region.
    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a count of fitting columns"
    )]
    pub fn fitting_columns(width: f32, card: f32, gap: f32) -> usize {
        (((width + gap) / (card + gap)).floor() as usize).max(1)
    }

    /// Fit uniform items to a bounded region. The caller provides the allowed widths
    /// in order, greatest first. At the floor the grid can overlap down to the press
    /// target. If those strips still do not fit, further rows go below the viewport.
    #[must_use]
    pub fn fit(
        count: usize,
        region: Vec2,
        aspect: f32,
        gap: f32,
        press: f32,
        widths: impl Iterator<Item = f32>,
    ) -> Self {
        let count = count.max(1);
        let mut grid = Self::new(1, Vec2::ONE, Vec2::splat(gap));
        for width in widths {
            let card = Vec2::new(width, width * aspect);
            let columns = Self::fitting_columns(region.x, width, gap);
            grid = Self::new(columns.min(count), card, Vec2::splat(gap));
            if grid.size(count).y <= region.y && width <= region.x {
                return grid;
            }
        }
        let card = grid.item;
        let lines = Self::fitting_columns(region.y, card.y, gap);
        let columns = count.div_ceil(lines);
        let pitch = Self::pitch(region.x, card.x, columns, gap);
        let (columns, pitch) = if pitch < press {
            let max = Self::fitting_columns(region.x, card.x, press - card.x);
            let columns = count.min(max);
            (columns, Self::pitch(region.x, card.x, columns, gap))
        } else {
            (columns, pitch)
        };
        Self {
            columns,
            item: card,
            step: Vec2::new(
                pitch,
                Self::pitch(region.y, card.y, count.div_ceil(columns), gap)
                    .max(press.min(card.y + gap)),
            ),
        }
    }

    /// Distance between starts of equally sized items sharing one axis. A single
    /// item can take the preferred spacing because no neighbour follows it.
    #[must_use]
    pub fn pitch(span: f32, item: f32, count: usize, gap: f32) -> f32 {
        if count > 1 {
            (item + gap).min(((span - item) / (count - 1) as f32).max(0.0))
        } else {
            item + gap
        }
    }

    /// Keeps a pile of cards in its reserved row while the gap can still expose
    /// the press target. More cards start a new row below it.
    #[must_use]
    pub fn reserved_row(
        count: usize,
        span: f32,
        item: Vec2,
        pitch: f32,
        gap: f32,
        press: f32,
    ) -> Self {
        let pitch = pitch.min(Self::pitch(span, item.x, count, gap));
        let (columns, pitch) = if pitch < press {
            let columns = count.min(Self::fitting_columns(span, item.x, press - item.x));
            (columns, pitch.max(press))
        } else {
            (count, pitch)
        };
        Self::new(columns, item, Vec2::new(pitch - item.x, gap))
    }

    /// The width of a box of height-to-width ratio `aspect` that fits the reserved
    /// columns and rows. A partial reservation can accommodate smaller items.
    #[must_use]
    pub fn fitting_width(region: Vec2, cells: Vec2, gap: Vec2, aspect: f32) -> f32 {
        let usable = region - (cells - Vec2::ONE).max(Vec2::ZERO) * gap;
        (usable.x / cells.x.max(1.0)).min(usable.y / (cells.y.max(1.0) * aspect))
    }

    /// The occupied size for `count` items. For centred rows this is the widest row.
    #[must_use]
    pub fn size(self, count: usize) -> Vec2 {
        if count == 0 {
            return Vec2::ZERO;
        }
        self.item
            + self.step
                * Vec2::new(
                    (count.min(self.columns) - 1) as f32,
                    (count.div_ceil(self.columns) - 1) as f32,
                )
    }

    /// Boxes in input order. Centred rows share the horizontal centre of `region`,
    /// and the whole grid is centred vertically when it fits. Excess rows follow
    /// below the region so an enclosing element can scroll and clip them.
    pub fn boxes(self, count: usize, region: Rect, centred: bool) -> impl Iterator<Item = Rect> {
        let mut cursor = Cursor::new(f32::INFINITY);
        let height = self.size(count).y;
        let y = region.y
            + if centred {
                ((region.height - height) / 2.0).max(0.0)
            } else {
                0.0
            };
        (0..count).map(move |index| {
            let at = cursor.place(self.item, self.columns, self.step - self.item);
            let start = if centred {
                let remaining = count - index / self.columns * self.columns;
                (region.width - self.size(remaining.min(self.columns)).x) / 2.0
            } else {
                0.0
            };
            Rect {
                x: region.x + start + at.x,
                y: y + at.y,
                width: self.item.x,
                height: self.item.y,
            }
        })
    }
}

/// A row-major cursor with its origin at the top left. Coordinates grow down.
pub(super) struct Cursor {
    width: f32,
    x: f32,
    y: f32,
    height: f32,
    column: usize,
}

impl Cursor {
    pub(super) fn new(width: f32) -> Self {
        Self {
            width,
            x: 0.0,
            y: 0.0,
            height: 0.0,
            column: 0,
        }
    }

    pub(super) fn place(&mut self, size: Vec2, columns: usize, gap: Vec2) -> Vec2 {
        if self.column > 0 && (self.column >= columns.max(1) || self.x + size.x > self.width) {
            self.x = 0.0;
            self.y += self.height + gap.y;
            self.height = 0.0;
            self.column = 0;
        }
        let corner = Vec2::new(self.x, self.y);
        self.x += size.x + gap.x;
        self.height = self.height.max(size.y);
        self.column += 1;
        corner
    }
}
