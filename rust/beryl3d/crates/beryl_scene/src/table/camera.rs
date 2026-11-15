//! The one perspective camera of a layout (docs/beryl-3d.md, "Space and camera"). Its pitch,
//! field of view and the point it looks at never change. A window of another shape moves it
//! along its line of sight, until the layout's required rectangle of the table fits inside the
//! window's safe area.

use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Mat4, Quat, Vec2, Vec3};

use super::{CARD_HEIGHT, Window};
use crate::pose::Pose;

/// A layout's camera, as data.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraData {
    /// The angle between the line of sight and straight down, in degrees.
    pub pitch_degrees: f32,
    /// The vertical field of view, in degrees.
    pub fov_degrees: f32,
    /// The point of the table the camera looks at, in millimetres.
    pub target: Vec3,
    /// The rectangle of the table (z = 0) that must always be visible: its lowest and highest
    /// corners, in millimetres.
    pub required: [Vec2; 2],
}

/// The nearest and farthest distances drawn, in millimetres.
const NEAR: f32 = 20.0;
const FAR: f32 = 20_000.0;
/// The range the fit searches for the camera's distance from its target, in millimetres.
const NEAREST: f32 = 100.0;
const FARTHEST: f32 = 15_000.0;
/// Bisection steps of the fit: 2^-32 of the range is far below a hundredth of a point.
const FIT_STEPS: u32 = 32;

/// The camera fitted to one window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub data: CameraData,
    pub window: Window,
    /// The distance from the eye to the target, in millimetres.
    pub distance: f32,
    pub eye: Vec3,
    pub view: Mat4,
    pub view_projection: Mat4,
    inverse_view: Mat4,
    tan_half: f32,
}

impl Camera {
    /// The camera for `data`, moved along its line of sight until the required rectangle fits
    /// inside `window`'s safe area.
    #[must_use]
    pub fn fit(data: CameraData, window: Window) -> Self {
        let mut camera = Self {
            data,
            window,
            distance: FARTHEST,
            eye: Vec3::ZERO,
            view: Mat4::IDENTITY,
            view_projection: Mat4::IDENTITY,
            inverse_view: Mat4::IDENTITY,
            tan_half: (data.fov_degrees.to_radians() / 2.0).tan(),
        };
        let (mut low, mut high) = (NEAREST, FARTHEST);
        for _ in 0..FIT_STEPS {
            let middle = f32::midpoint(low, high);
            camera.set_distance(middle);
            if camera.required_fits() {
                high = middle;
            } else {
                low = middle;
            }
        }
        camera.set_distance(high);
        camera
    }

    /// Whether every corner of the required rectangle lies inside the safe area.
    #[must_use]
    pub fn required_fits(&self) -> bool {
        let [low, high] = self.data.required;
        let size = self.window.size;
        let inset = self.window.safe_area;
        [
            low,
            Vec2::new(high.x, low.y),
            high,
            Vec2::new(low.x, high.y),
        ]
        .into_iter()
        .all(|corner| {
            let p = self.to_screen(corner.extend(0.0));
            p.x >= inset.left
                && p.x <= size.width - inset.right
                && p.y >= inset.top
                && p.y <= size.height - inset.bottom
        })
    }

    fn set_distance(&mut self, distance: f32) {
        let pitch = self.data.pitch_degrees.to_radians();
        let target = self.data.target;
        let aspect = self.window.size.width / self.window.size.height;
        self.distance = distance;
        self.eye = target + Vec3::new(0.0, -pitch.sin(), pitch.cos()) * distance;
        self.view = look_at_mat4(self.eye, target, Vec3::Y);
        let projection =
            directx::perspective(self.data.fov_degrees.to_radians(), aspect, NEAR, FAR);
        self.view_projection = projection * self.view;
        self.inverse_view = self.view.inverse();
    }

    /// A point of the table's space in normalized device coordinates: x and y from -1 to 1, y
    /// up, and z the depth from 0 at the near plane to 1 at the far one.
    #[must_use]
    pub fn project(&self, point: Vec3) -> Vec3 {
        self.view_projection.project_point3(point)
    }

    /// A point of the table's space on the window, in logical points from its top left corner.
    #[must_use]
    pub fn to_screen(&self, point: Vec3) -> Vec2 {
        let ndc = self.project(point);
        let size = self.window.size;
        Vec2::new(
            f32::midpoint(ndc.x, 1.0) * size.width,
            f32::midpoint(1.0, -ndc.y) * size.height,
        )
    }

    /// The distance of `point` in front of the eye, along the line of sight, in millimetres.
    #[must_use]
    pub fn depth(&self, point: Vec3) -> f32 {
        -self.view.transform_point3(point).z
    }

    /// How many logical points one millimetre along x measures at `point`. Along a line of
    /// constant y and z the camera's depth is constant, so this holds for a whole row.
    #[must_use]
    pub fn points_per_millimetre(&self, point: Vec3) -> f32 {
        self.to_screen(point + Vec3::X).x - self.to_screen(point).x
    }

    /// A pose given relative to the camera, so that it keeps its size and place on screen at
    /// any window shape: at `ndc` on the screen, `depth` millimetres in front of the eye,
    /// turned by `rotation` from facing the camera, and scaled so that a card is `height` (a
    /// fraction of the window's height) tall.
    #[must_use]
    pub fn screen_pose(&self, ndc: Vec2, depth: f32, rotation: Quat, height: f32) -> Pose {
        let aspect = self.window.size.width / self.window.size.height;
        let local = Vec3::new(
            ndc.x * self.tan_half * aspect * depth,
            ndc.y * self.tan_half * depth,
            -depth,
        );
        Pose {
            position: self.inverse_view.transform_point3(local),
            rotation: Quat::from_mat4(&self.inverse_view) * rotation,
            scale: height * 2.0 * depth * self.tan_half / CARD_HEIGHT,
            ..Pose::default()
        }
    }
}
