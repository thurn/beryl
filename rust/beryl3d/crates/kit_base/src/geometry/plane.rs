//! The plane on which two-dimensional elements are drawn through a scene camera.

use glam::{Affine3A, Quat, Vec2, Vec3};

use super::{Rect, Window, zoom_rect};

/// Projects a box in logical window points onto the camera's flat drawing plane.
#[derive(Clone, Copy, Debug)]
pub struct ScreenPlane {
    inverse_view: Affine3A,
    window: Window,
    half_height: f32,
    depth: f32,
    zoom: Zoom,
}

impl ScreenPlane {
    /// The plane at `depth` in front of the camera. `half_height` is its vertical
    /// half extent at that distance.
    #[must_use]
    pub fn new(inverse_view: Affine3A, window: Window, half_height: f32, depth: f32) -> Self {
        Self {
            inverse_view,
            window,
            half_height,
            depth,
            zoom: Zoom::IDENTITY,
        }
    }

    /// Physical pixels per logical point.
    #[must_use]
    pub fn scale(&self) -> f32 {
        self.window.scale
    }

    /// This plane with the flat layer scaled by `scale` about `pivot`, a point of the window in
    /// logical points, as CSS `scale` scales an element about its centre.
    #[must_use]
    pub fn zoomed(self, pivot: Vec2, scale: f32) -> Self {
        Self {
            zoom: Zoom::around(pivot, scale),
            ..self
        }
    }

    /// Uses the same visual transform as the flat layer's hit boxes.
    #[must_use]
    pub fn with_zoom(self, zoom: Zoom) -> Self {
        Self { zoom, ..self }
    }

    /// The transform that lays the unit quad over `rect`, in logical points from the window's
    /// top left corner, on the flat layer's plane (docs/beryl-3d.md, "The frame": one projection
    /// serves the table and the flat layer).
    #[must_use]
    pub fn layer_transform(&self, rect: Rect) -> Affine3A {
        let rect = self.zoom.rect(rect);
        let size = self.window.size;
        let half_height = self.half_height;
        let half_width = half_height * size.width / size.height;
        let centre = Vec2::new(
            (rect.x + rect.width / 2.0) / size.width * 2.0 - 1.0,
            1.0 - (rect.y + rect.height / 2.0) / size.height * 2.0,
        );
        let local = Affine3A::from_scale_rotation_translation(
            Vec3::new(
                rect.width / size.width * 2.0 * half_width,
                rect.height / size.height * 2.0 * half_height,
                1.0,
            ),
            Quat::IDENTITY,
            Vec3::new(centre.x * half_width, centre.y * half_height, -self.depth),
        );
        self.inverse_view * local
    }
}

/// Composes uniform, centre-relative scales for flat elements nested in another element.
#[derive(Clone, Copy, Debug)]
pub struct Zoom {
    outer: (Vec2, f32),
    offset: Vec2,
    scale: f32,
}

impl Zoom {
    pub const IDENTITY: Self = Self::around(Vec2::ZERO, 1.0);

    #[must_use]
    pub const fn around(pivot: Vec2, scale: f32) -> Self {
        Self {
            outer: (pivot, scale),
            offset: Vec2::ZERO,
            scale: 1.0,
        }
    }

    /// Adds a scale in the coordinate system before this transform.
    #[must_use]
    pub fn inside(self, pivot: Vec2, scale: f32) -> Self {
        Self {
            offset: self.offset + pivot * ((1.0 - scale) * self.scale),
            scale: self.scale * scale,
            ..self
        }
    }

    #[must_use]
    pub fn rect(self, mut rect: Rect) -> Rect {
        if self.scale.to_bits() != 1.0_f32.to_bits() || self.offset != Vec2::ZERO {
            rect = zoom_rect(rect, (Vec2::ZERO, self.scale));
            rect.x += self.offset.x;
            rect.y += self.offset.y;
        }
        zoom_rect(rect, self.outer)
    }
}
