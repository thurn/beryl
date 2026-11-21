//! Point queries over scene nodes, including layout between two finished frames.

use glam::Vec2;
use kit_base::geometry::Rect;

/// A projected or laid-out region and the clip that limits it, in logical points.
#[derive(Clone, Copy)]
pub enum Area {
    Quad([Vec2; 4], Option<Rect>),
    Rect(Rect, Option<Rect>),
}

/// Finds the frontmost candidate at `point` that `accept` wants. The visitor supplies its
/// candidates in reverse paint order and stops at the first successful test.
pub fn hit_test<T, C>(
    point: Vec2,
    visit: impl FnOnce(&dyn Fn(Area, C) -> Option<T>) -> Option<T>,
    accept: impl Fn(C) -> Option<T>,
) -> Option<T> {
    visit(&|area, candidate| area.contains(point).then(|| accept(candidate)).flatten())
}

impl Area {
    /// Whether the point lies within both the shape and its clip.
    #[must_use]
    pub fn contains(self, point: Vec2) -> bool {
        match self {
            Self::Quad(corners, clip) => {
                clip.is_none_or(|clip| contains(clip, point)) && in_quad(corners, point)
            }
            Self::Rect(rect, clip) => {
                contains(rect, point) && clip.is_none_or(|clip| contains(clip, point))
            }
        }
    }

    /// Intersects a clipping node's box with the clip it inherited.
    #[must_use]
    pub fn clip(rect: Rect, inherited: Option<Rect>) -> Rect {
        let Some(clip) = inherited else {
            return rect;
        };
        let x = rect.x.max(clip.x);
        let y = rect.y.max(clip.y);
        Rect {
            x,
            y,
            width: (rect.x + rect.width).min(clip.x + clip.width) - x,
            height: (rect.y + rect.height).min(clip.y + clip.height) - y,
        }
    }
}

fn contains(rect: Rect, point: Vec2) -> bool {
    rect.width > 0.0
        && rect.height > 0.0
        && point.x >= rect.x
        && point.y >= rect.y
        && point.x <= rect.x + rect.width
        && point.y <= rect.y + rect.height
}

/// Tests a convex quadrilateral in either winding.
fn in_quad(corners: [Vec2; 4], point: Vec2) -> bool {
    let mut side = None;
    for index in 0..4 {
        let a = corners[index];
        let b = corners[(index + 1) % 4];
        let cross = (b - a).perp_dot(point - a);
        if cross.abs() <= f32::EPSILON {
            continue;
        }
        let left = cross > 0.0;
        if *side.get_or_insert(left) != left {
            return false;
        }
    }
    side.is_some()
}
