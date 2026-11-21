//! Window measurements and the breakpoint shared by layout and interface checks.

use crate::input::{Insets, Size};

/// The shorter side, in logical points, below which a window uses the phone layout.
pub const PHONE_BELOW_POINTS: f32 = crate::data::space::flat::PHONE_UNDER;

pub(crate) fn uses_phone_layout(size: Size) -> bool {
    size.width.min(size.height) < PHONE_BELOW_POINTS
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

impl Window {
    /// Whether the window takes the phone layout: its shorter side is under 500 points
    /// (docs/beryl-3d.md, "Sizes").
    #[must_use]
    pub fn phone(self) -> bool {
        uses_phone_layout(self.size)
    }
}
