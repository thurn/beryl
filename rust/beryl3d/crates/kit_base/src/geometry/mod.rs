//! Surface geometry, and the dimensions of the card mesh.

pub mod plane;

use glam::Vec2;

use crate::data::profiles::SizeClass;

/// A rectangle with its top left corner at (`x`, `y`). It is in logical points unless the field
/// holding it says otherwise (a frame's clips are in physical pixels).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A size in logical points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    /// Project a logical point from the top left into normalized device coordinates.
    #[must_use]
    pub fn ndc(self, x: f32, y: f32) -> Vec2 {
        Vec2::new(x / self.width * 2.0 - 1.0, 1.0 - y / self.height * 2.0)
    }

    /// Project normalized device coordinates into logical points from the top left.
    #[must_use]
    pub fn screen(self, at: Vec2) -> Vec2 {
        Vec2::new(
            f32::midpoint(at.x, 1.0) * self.width,
            f32::midpoint(1.0, -at.y) * self.height,
        )
    }
}

/// A band of the window, in logical points from its top edge.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Band {
    pub top: f32,
    pub height: f32,
}

impl Band {
    #[must_use]
    pub fn between(top: f32, bottom: f32) -> Self {
        Self {
            top,
            height: bottom - top,
        }
    }

    #[must_use]
    pub fn bottom(self) -> f32 {
        self.top + self.height
    }

    #[must_use]
    pub fn middle(self) -> f32 {
        self.top + self.height / 2.0
    }
}

/// Which screen axis advances through an arranged container.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollAxis {
    Horizontal,
    Vertical,
}

/// A viewport into an ordered list of groups.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupWindow {
    /// The first group in view.
    pub first: usize,
    /// The number of visible groups.
    pub shown: usize,
    /// The largest requested first group that the container can accept.
    pub last: usize,
    pub axis: ScrollAxis,
}

/// How far in from each edge of the surface its clear area starts, in logical points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Insets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

/// The window the table is drawn in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    pub size: Size,
    /// Physical pixels per logical point.
    pub scale: f32,
    pub safe_area: Insets,
    /// The host data's selected size class, once the client has received this window.
    pub size_class: Option<SizeClass>,
}

impl Default for Window {
    fn default() -> Self {
        Self {
            size: Size {
                width: 1440.0,
                height: 900.0,
            },
            scale: 1.0,
            safe_area: Insets::default(),
            size_class: None,
        }
    }
}

/// `rect` scaled by `zoom`'s scale about its point.
pub fn zoom_rect(rect: Rect, (pivot, scale): (Vec2, f32)) -> Rect {
    Rect {
        x: pivot.x + (rect.x - pivot.x) * scale,
        y: pivot.y + (rect.y - pivot.y) * scale,
        width: rect.width * scale,
        height: rect.height * scale,
    }
}
