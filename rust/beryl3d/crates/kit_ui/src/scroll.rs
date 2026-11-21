//! Scroll bounds and offset calculations for clipped elements and arranged containers.
//! A viewport reports its offset and bounds in logical points or whole groups. The owner
//! applies the returned offset.

use glam::Vec2;
use kit_base::geometry::{GroupWindow, ScrollAxis};

use crate::input::{Key, KeyCode, KeyPhase, Modifiers};

/// The bounds and current offset of a scroller, plus the amount visible for page keys.
#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    pub offset: Vec2,
    limit: Vec2,
    page: Vec2,
    grouped: bool,
}

impl Viewport {
    #[must_use]
    pub fn points(offset: Vec2, limit: Vec2, page: Vec2) -> Self {
        Self {
            offset,
            limit,
            page,
            grouped: false,
        }
    }

    /// A viewport over the whole groups an arrangement shows.
    #[must_use]
    pub fn groups(window: GroupWindow) -> Self {
        let along = match window.axis {
            ScrollAxis::Horizontal => Vec2::X,
            ScrollAxis::Vertical => Vec2::Y,
        };
        Self {
            offset: along * window.first as f32,
            limit: along * window.last as f32,
            page: along * window.shown as f32,
            grouped: true,
        }
    }

    #[must_use]
    pub fn is_grouped(self) -> bool {
        self.grouped
    }

    pub fn axes(self) -> Vec2 {
        Vec2::select(self.limit.cmpgt(Vec2::ZERO), Vec2::ONE, Vec2::ZERO)
    }

    pub fn clamp(self, offset: Vec2) -> Vec2 {
        offset.clamp(Vec2::ZERO, self.limit)
    }

    /// Whether this wheel event can scroll one of the viewport's axes.
    #[must_use]
    pub fn takes_wheel(self, wheel: Vec2, notch: f32) -> bool {
        self.wheel(wheel, notch) != Vec2::ZERO
    }

    pub fn wheel(self, wheel: Vec2, notch: f32) -> Vec2 {
        if self.grouped {
            self.axes() * -(wheel.x + wheel.y) / notch
        } else {
            -wheel * self.axes()
        }
    }

    /// The requested offset for a plain scrolling key on this viewport.
    #[must_use]
    pub fn key(self, key: Key, step_points: f32) -> Option<Vec2> {
        if key.phase == KeyPhase::Up || key.modifiers != Modifiers::default() {
            return None;
        }
        let step = if self.grouped { 1.0 } else { step_points };
        let vertical = if self.limit.y > 0.0 { Vec2::Y } else { Vec2::X };
        let delta = match key.code {
            KeyCode::ArrowLeft => -Vec2::X * step,
            KeyCode::ArrowRight => Vec2::X * step,
            KeyCode::ArrowUp => -Vec2::Y * step,
            KeyCode::ArrowDown => Vec2::Y * step,
            KeyCode::PageUp => -vertical * self.page,
            KeyCode::PageDown => vertical * self.page,
            KeyCode::Home => -vertical * self.limit,
            KeyCode::End => vertical * self.limit,
            _ => return None,
        };
        (delta * self.axes() != Vec2::ZERO).then(|| self.clamp(self.offset + delta))
    }

    pub fn offset_for_travel(self, origin: Vec2, travel: Vec2, group_step: f32) -> Vec2 {
        let units = if self.grouped {
            (travel / group_step).trunc()
        } else {
            travel
        };
        self.clamp(origin + units)
    }
}
