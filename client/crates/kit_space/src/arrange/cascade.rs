//! Cascade geometry for rectangles facing the camera. Their projected edges are parallel
//! to the screen, so a strip or exposed corner can be measured directly in the region's units.

use glam::Vec2;
use kit_base::geometry::{GroupWindow, ScrollAxis};

use super::Overflow;

/// A cascade of camera-facing cards. A horizontal cascade uses `step` across the
/// region. An anchored cascade uses `step` up the region, with `direction.x` units
/// of horizontal advance for each vertical unit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cascade {
    pub direction: Vec2,
    pub step: f32,
}

/// A cascade that rises from a fixed lower corner. All distances are measured in the same
/// projected units, with y up. The left edge lets wrapped rows extend past the preferred reach
/// while keeping the cards inside the window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnchoredCascade {
    pub foot: Vec2,
    pub ceiling: f32,
    pub left: f32,
    pub size: Vec2,
    pub reach: f32,
    pub pitch: f32,
    pub least_scale: f32,
}

/// The result for equally sized, camera-facing items. The window starts at the front item
/// unless scrolled. Hidden items have their rest at the nearest end of the visible window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CascadeLayout {
    pub window: GroupWindow,
    pub size: Vec2,
    pub overflow: Overflow,
    origin: Vec2,
    step: Vec2,
    wrap: Option<usize>,
}

impl CascadeLayout {
    /// The centre and visibility of item `index`, in front-to-back order.
    #[must_use]
    pub fn item(self, index: usize) -> (Vec2, bool) {
        let first = self.window.first;
        let end = first + self.window.shown;
        let offset = index.clamp(first, end.saturating_sub(1).max(first)) - first;
        let advance = self.wrap.map_or_else(
            || self.step * offset as f32,
            |columns| self.step * Vec2::new((offset % columns) as f32, (offset / columns) as f32),
        );
        (self.origin + advance, (first..end).contains(&index))
    }

    /// The visible cards' bounds in the same coordinates as their centres.
    #[must_use]
    pub fn bounds(self) -> Option<(Vec2, Vec2)> {
        let last = self.window.shown.checked_sub(1)?;
        let far = self.wrap.map_or_else(
            || self.step * last as f32,
            |columns| self.step * Vec2::new(last.min(columns - 1) as f32, (last / columns) as f32),
        );
        let half = self.size / 2.0;
        Some((
            self.origin + far.min(Vec2::ZERO) - half,
            self.origin + far.max(Vec2::ZERO) + half,
        ))
    }
}

fn window(count: usize, capacity: usize, offset: usize, axis: ScrollAxis) -> GroupWindow {
    let shown = count.min(capacity);
    let last = count - shown;
    GroupWindow {
        first: offset.min(last),
        shown,
        last,
        axis,
    }
}

impl Cascade {
    /// Arrange a horizontal cascade centred in `span`. When all the items do not fit, keep
    /// `gutter` free at both ends for the counts of items outside the window.
    #[must_use]
    pub fn centred(
        self,
        (left, right, middle): (f32, f32, f32),
        size: Vec2,
        (target, gutter): (f32, f32),
        (count, offset): (usize, usize),
    ) -> CascadeLayout {
        let step = self.step.max(target);
        let room = right - left - size.x;
        let full = if room >= 0.0 && size.min_element() >= target {
            slots(room, step)
        } else {
            0
        };
        let capacity = if full > 0 && count > full {
            slots(room - 2.0 * gutter, step)
        } else {
            full
        };
        let window = window(count, capacity, offset, ScrollAxis::Horizontal);
        let step = Vec2::X * self.direction.x.signum() * step;
        CascadeLayout {
            window,
            size,
            overflow: if count > capacity {
                Overflow::Scrolled
            } else {
                Overflow::FullSize
            },
            origin: Vec2::new(f32::midpoint(left, right), middle)
                - step * (window.shown.saturating_sub(1) as f32 / 2.0),
            step,
            wrap: None,
        }
    }

    /// The rectangle reserved for the diagonal cascade, from the full band's lower left
    /// to its upper right. Other containers can reserve it before any items arrive.
    #[must_use]
    pub fn reservation(self, region: AnchoredCascade) -> (Vec2, Vec2) {
        let scale = ((region.ceiling - region.foot.y) / (region.size.y + region.reach)).min(1.0);
        let left = region.foot.x - (region.size.x / 2.0 - self.direction.x * region.reach) * scale;
        (
            Vec2::new(left, region.foot.y),
            Vec2::new(region.foot.x + region.size.x / 2.0, region.ceiling),
        )
    }

    /// Keep the front item at the foot of the region. Try the preferred diagonal step,
    /// compress it to the available reach, then shrink the items if needed. When the
    /// diagonal cannot leave a press square, wrap into rows and expose a square at each
    /// item's upper left. The window is used only when the rows also fill the region.
    #[must_use]
    pub fn anchored(
        self,
        region: AnchoredCascade,
        target: f32,
        (count, offset): (usize, usize),
    ) -> CascadeLayout {
        let room = region.ceiling - region.foot.y;
        let gaps = count.saturating_sub(1) as f32;
        let step = self.step.min(region.reach / gaps.max(1.0));
        let rise = gaps * step;
        let scale = (room / (region.size.y + rise)).min(1.0);
        let diagonal = Vec2::new(self.direction.x, 1.0) * (step * scale);
        let size = region.size * scale;
        let reaches = region.foot.x + diagonal.x * gaps - size.x / 2.0 >= region.left;
        if size.min_element() >= target
            && scale >= region.least_scale
            && reaches
            && (count <= 1 || diagonal.abs().max_element() >= target)
        {
            return CascadeLayout {
                window: window(count, count, offset, ScrollAxis::Vertical),
                size,
                overflow: if scale < 1.0 {
                    Overflow::Scaled
                } else if step < self.step {
                    Overflow::Overlapped
                } else {
                    Overflow::FullSize
                },
                origin: region.foot + Vec2::Y * size.y / 2.0,
                step: diagonal,
                wrap: None,
            };
        }
        self.wrapped(region, target, (count, offset))
    }

    fn wrapped(
        self,
        region: AnchoredCascade,
        target: f32,
        (count, offset): (usize, usize),
    ) -> CascadeLayout {
        let room = region.ceiling - region.foot.y;
        let preferred = region.foot.x - self.reservation(region).0.x;
        let available = region.foot.x - region.left;
        let pitch = region.pitch.max(target);
        let least = region.least_scale.max(target / region.size.min_element());
        let scale_for = |columns| {
            let rows = count.div_ceil(columns);
            let across = (preferred - (columns - 1) as f32 * pitch) / (region.size.x / 2.0);
            let up = (room - rows.saturating_sub(1) as f32 * pitch) / region.size.y;
            across.min(up).min(1.0)
        };
        let longest = slots(preferred, pitch).min(count.max(1));
        let (mut columns, mut scale) = (1..=longest)
            .map(|columns| (columns, scale_for(columns)))
            .fold((1, f32::NEG_INFINITY), |best, candidate| {
                if candidate.1 > best.1 {
                    candidate
                } else {
                    best
                }
            });
        let mut capacity = count;
        let fits = region.size * scale;
        if scale < least || fits.x / 2.0 + (columns - 1) as f32 * pitch > available {
            scale = least.min(1.0);
            let rows = slots(room - region.size.y * scale, pitch);
            let widest = slots(available - region.size.x * scale / 2.0, pitch);
            capacity = if region.size.y * scale <= room
                && region.size.x * scale / 2.0 <= available
                && region.size.min_element() * scale >= target
            {
                rows.saturating_mul(widest)
            } else {
                0
            };
            columns = count.min(capacity).div_ceil(rows).max(1);
        }
        let size = region.size * scale;
        CascadeLayout {
            window: window(count, capacity, offset, ScrollAxis::Vertical),
            size,
            overflow: if capacity < count {
                Overflow::Scrolled
            } else if scale < 1.0 {
                Overflow::Scaled
            } else {
                Overflow::Overlapped
            },
            origin: region.foot + Vec2::Y * size.y / 2.0,
            step: Vec2::new(-pitch, pitch),
            wrap: Some(columns),
        }
    }
}

/// How many starts fit when the first item is already accounted for.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the nonnegative quotient is rounded down to a count of full steps"
)]
fn slots(room: f32, step: f32) -> usize {
    ((room.max(0.0) / step) as usize).saturating_add(1)
}
