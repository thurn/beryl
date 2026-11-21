//! Surface geometry, and the dimensions of the card mesh.

pub mod plane;

use glam::Vec2;

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
        }
    }
}

/// A card's printed width and height (`card` in `space.json`).
pub const CARD_WIDTH: f32 = crate::data::space::card::WIDTH;
pub const CARD_HEIGHT: f32 = crate::data::space::card::HEIGHT;

/// `rect` scaled by `zoom`'s scale about its point.
pub fn zoom_rect(rect: Rect, (pivot, scale): (Vec2, f32)) -> Rect {
    Rect {
        x: pivot.x + (rect.x - pivot.x) * scale,
        y: pivot.y + (rect.y - pivot.y) * scale,
        width: rect.width * scale,
        height: rect.height * scale,
    }
}

impl Window {
    /// Whether the window uses the compact phone layout.
    #[must_use]
    pub fn phone(self) -> bool {
        phone_size(self.size)
    }
}

/// The shorter side below which the phone layout fits the window.
pub const PHONE_BELOW_POINTS: f32 = crate::data::space::flat::PHONE_UNDER;

/// Whether the shorter side of `size` calls for the phone layout.
#[must_use]
pub fn phone_size(size: Size) -> bool {
    size.width.min(size.height) < PHONE_BELOW_POINTS
}
