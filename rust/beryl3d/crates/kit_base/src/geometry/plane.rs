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
    zoom: (Vec2, f32),
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
            zoom: (Vec2::ZERO, 1.0),
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
            zoom: (pivot, scale),
            ..self
        }
    }

    /// The transform that lays the unit quad over `rect`, in logical points from the window's
    /// top left corner, on the flat layer's plane (docs/beryl-3d.md, "The frame": one projection
    /// serves the table and the flat layer).
    #[must_use]
    pub fn layer_transform(&self, rect: Rect) -> Affine3A {
        let rect = zoom_rect(rect, self.zoom);
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
