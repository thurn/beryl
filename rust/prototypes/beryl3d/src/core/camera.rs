//! The one perspective camera: a fixed pitch and field of view, looking at a fixed point of the
//! table, moved along its line of sight until a rectangle of the table fits the window
//! (docs/beryl-3d.md, "Space and camera").

use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Mat4, Quat, Vec2, Vec3, Vec4};

use super::motion::Pose;

/// The angle between the camera's line of sight and straight down, fitted by eye to Arena's
/// recordings: the far edge of Arena's table is about 0.85 times as wide on screen as the near.
const PITCH_DEGREES: f32 = 24.0;
const FOV_DEGREES: f32 = 30.0;
/// The point of the table the camera looks at, in millimetres.
const TARGET: Vec3 = Vec3::new(0.0, -30.0, 0.0);
/// The rectangle of the table that must always be visible: both players' rows and piles, and
/// below them the strip the viewer's hand covers.
const VISIBLE: [Vec3; 4] = [
    Vec3::new(-470.0, -290.0, 0.0),
    Vec3::new(470.0, -290.0, 0.0),
    Vec3::new(470.0, 215.0, 0.0),
    Vec3::new(-470.0, 215.0, 0.0),
];
const NEAR: f32 = 20.0;
const FAR: f32 = 20_000.0;

pub struct Camera {
    pub width: u32,
    pub height: u32,
    pub aspect: f32,
    pub distance: f32,
    pub eye: Vec3,
    pub view: Mat4,
    pub view_proj: Mat4,
    inverse_view: Mat4,
    inverse_rotation: Quat,
    tan_half: f32,
}

impl Camera {
    pub fn new(width: u32, height: u32) -> Camera {
        let mut camera = Camera {
            width,
            height,
            aspect: 1.0,
            distance: 1000.0,
            eye: Vec3::ZERO,
            view: Mat4::IDENTITY,
            view_proj: Mat4::IDENTITY,
            inverse_view: Mat4::IDENTITY,
            inverse_rotation: Quat::IDENTITY,
            tan_half: (FOV_DEGREES.to_radians() / 2.0).tan(),
        };
        camera.fit(width, height);
        camera
    }

    /// Chooses the distance at which [`VISIBLE`] fits inside the window, by bisection.
    pub fn fit(&mut self, width: u32, height: u32) {
        self.width = width.max(1);
        self.height = height.max(1);
        self.aspect = self.width as f32 / self.height as f32;
        let (mut low, mut high) = (100.0_f32, 20_000.0_f32);
        for _ in 0..40 {
            let middle = (low + high) / 2.0;
            self.set_distance(middle);
            let fits = VISIBLE.iter().all(|&corner| {
                let p = self.project(corner);
                p.x.abs() <= 0.98 && p.y.abs() <= 0.98
            });
            if fits {
                high = middle;
            } else {
                low = middle;
            }
        }
        self.set_distance(high);
    }

    fn set_distance(&mut self, distance: f32) {
        let pitch = PITCH_DEGREES.to_radians();
        self.distance = distance;
        self.eye = TARGET + Vec3::new(0.0, -pitch.sin(), pitch.cos()) * distance;
        self.view = look_at_mat4(self.eye, TARGET, Vec3::Y);
        let projection = directx::perspective(FOV_DEGREES.to_radians(), self.aspect, NEAR, FAR);
        self.view_proj = projection * self.view;
        self.inverse_view = self.view.inverse();
        self.inverse_rotation = Quat::from_mat4(&self.inverse_view);
    }

    /// A world point in normalized device coordinates (x and y in -1..1, z the depth).
    pub fn project(&self, point: Vec3) -> Vec3 {
        let clip = self.view_proj * point.extend(1.0);
        clip.truncate() / clip.w.max(1e-6)
    }

    pub fn clip(&self, point: Vec3) -> Vec4 {
        self.view_proj * point.extend(1.0)
    }

    /// Distance from the camera along its line of sight.
    pub fn depth(&self, point: Vec3) -> f32 {
        -(self.view.transform_point3(point).z)
    }

    /// The ray from the eye through a point of the screen.
    pub fn ray(&self, ndc: Vec2) -> (Vec3, Vec3) {
        let local = Vec3::new(
            ndc.x * self.tan_half * self.aspect,
            ndc.y * self.tan_half,
            -1.0,
        );
        (
            self.eye,
            self.inverse_view.transform_vector3(local).normalize(),
        )
    }

    /// A pose given relative to the camera: at screen point `ndc`, `depth` millimetres in front
    /// of the eye, turned by `rotation` from facing the camera, at `height` (a fraction of the
    /// screen's height) for a card's height. Hands use it so that they keep their size and
    /// place at any window shape.
    pub fn screen_pose(&self, ndc: Vec2, depth: f32, rotation: Quat, height: f32) -> Pose {
        let local = Vec3::new(
            ndc.x * self.tan_half * self.aspect * depth,
            ndc.y * self.tan_half * depth,
            -depth,
        );
        let scale = height * 2.0 * depth * self.tan_half / super::draw::CARD_H;
        Pose {
            position: self.inverse_view.transform_point3(local),
            rotation: self.inverse_rotation * rotation,
            scale,
            opacity: 1.0,
        }
    }
}
