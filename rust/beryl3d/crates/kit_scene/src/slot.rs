//! Named feature regions are resolved to boxes in logical points.

use kit_base::data::profiles::{Align, SlotSpec};
use kit_base::geometry::{Rect, Window};

pub use kit_base::data::profiles::SizeClass;

/// The box available to a feature inside the window's safe area.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    pub rect: Rect,
}

impl Slot {
    /// The usable part of the window in logical points.
    #[must_use]
    pub fn safe_area(window: Window) -> Rect {
        let inset = window.safe_area;
        Rect {
            x: inset.left,
            y: inset.top,
            width: (window.size.width - inset.left - inset.right).max(0.0),
            height: (window.size.height - inset.top - inset.bottom).max(0.0),
        }
    }

    /// Fits a slot within its region, clipped to the safe area first.
    #[must_use]
    pub fn fit(spec: SlotSpec, region: Rect, safe: Rect) -> Self {
        let [top, right, bottom, left] = spec.inset;
        let (x, width) = fit_axis(
            (region.x, region.width),
            (safe.x, safe.width),
            (left, right),
            spec.max_width,
            spec.align_x,
        );
        let (y, height) = fit_axis(
            (region.y, region.height),
            (safe.y, safe.height),
            (top, bottom),
            spec.max_height,
            spec.align_y,
        );
        Self {
            rect: Rect {
                x,
                y,
                width,
                height,
            },
        }
    }
}

fn fit_axis(
    (start, length): (f32, f32),
    (safe_start, safe_length): (f32, f32),
    (leading, trailing): (f32, f32),
    limit: Option<f32>,
    align: Align,
) -> (f32, f32) {
    let safe_end = safe_start + safe_length;
    let end = (start + length).clamp(safe_start, safe_end);
    let start = (start.clamp(safe_start, end) + leading).min(end);
    let room = (end - start - trailing).max(0.0);
    let length = limit.map_or(room, |limit| room.min(limit));
    let spare = room - length;
    let offset = match align {
        Align::Start => 0.0,
        Align::Center => spare * 0.5,
        Align::End => spare,
    };
    (start + offset, length)
}
