//! The one perspective camera of a layout (docs/beryl-3d.md, "Space and camera"). Its pitch,
//! field of view and the point it looks at never change. A window of another shape moves it
//! along its line of sight, until the layout's required rectangle of the table fits inside the
//! window's safe area.

use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Affine3A, Mat3, Mat4, Quat, Vec2, Vec3};

use kit_base::geometry::Window;
use kit_base::geometry::{Rect, plane::ScreenPlane};
use kit_base::pose::Pose;

pub use kit_base::data::profiles::CameraData;

/// The nearest and farthest distances drawn, in millimetres.
const NEAR: f32 = 20.0;
const FAR: f32 = 20_000.0;
/// How far in front of the eye the flat layer's plane lies, in millimetres: just past the near
/// plane, so that it is in front of everything on the table.
const LAYER_DEPTH: f32 = NEAR * 1.5;
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
    /// The part of the window the required rectangle is fitted inside, in logical points: the
    /// safe area, or the region a layout reserves for the table.
    pub area: Rect,
    pub eye: Vec3,
    pub view: Mat4,
    pub view_projection: Mat4,
    inverse_view: Mat4,
    inverse_rotation: Quat,
    tan_half: f32,
}

impl Camera {
    /// The camera for `data`, moved along its line of sight until the required rectangle fits
    /// inside `window`'s safe area.
    #[must_use]
    pub fn fit(data: CameraData, window: Window) -> Self {
        let (size, inset) = (window.size, window.safe_area);
        let area = Rect {
            x: inset.left,
            y: inset.top,
            width: size.width - inset.left - inset.right,
            height: size.height - inset.top - inset.bottom,
        };
        Self::fit_inside(data, window, area)
    }

    /// The camera for `data`, moved along its line of sight until the required rectangle fits
    /// inside `area`, a part of `window` in logical points from its top left corner. The target
    /// stays at the window's middle, so the rectangle is not centred in an area that is off the
    /// window's middle: the phone layout's area is, and its target is chosen so that the fitted
    /// rectangle fills that area at 393 by 759.
    #[must_use]
    pub fn fit_inside(data: CameraData, window: Window, area: Rect) -> Self {
        let mut camera = Self {
            data,
            window,
            area,
            distance: FARTHEST,
            eye: Vec3::ZERO,
            view: Mat4::IDENTITY,
            view_projection: Mat4::IDENTITY,
            inverse_view: Mat4::IDENTITY,
            inverse_rotation: Quat::IDENTITY,
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
        camera.inverse_rotation = Quat::from_mat4(&camera.inverse_view);
        camera
    }

    /// Whether every corner of the required rectangle lies inside the area it is fitted in.
    #[must_use]
    pub fn required_fits(&self) -> bool {
        let area = self.area;
        self.required_corners().into_iter().all(|p| {
            p.x >= area.x
                && p.x <= area.x + area.width
                && p.y >= area.y
                && p.y <= area.y + area.height
        })
    }

    /// The corners of the required rectangle on the window, in logical points.
    #[must_use]
    pub fn required_corners(&self) -> [Vec2; 4] {
        let [low, high] = self.data.required;
        [
            low,
            Vec2::new(high.x, low.y),
            high,
            Vec2::new(low.x, high.y),
        ]
        .map(|corner| self.to_screen(corner.extend(0.0)))
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
            rotation: self.inverse_rotation * rotation,
            scale: height * 2.0 * depth * self.tan_half / self.data.card.height,
            ..Pose::default()
        }
    }

    /// Moves a pose by logical points along the screen at its current depth.
    pub fn move_in_screen(&self, pose: &mut Pose, delta: Vec2) {
        let units = 2.0 * self.depth(pose.position) * self.tan_half / self.window.size.height;
        pose.position += self.inverse_rotation * Vec3::new(delta.x, -delta.y, 0.0) * units;
    }

    /// A card's corners on screen, in logical points, when a card of the camera's card size is
    /// laid by `transform`:
    /// bottom left, bottom right, top right and top left of its face.
    #[must_use]
    pub fn card_corners(&self, transform: Affine3A) -> [Vec2; 4] {
        let Vec2 { x, y } = self.data.card.half();
        [(-x, -y), (x, -y), (x, y), (-x, y)]
            .map(|(x, y)| self.to_screen(transform.transform_point3(Vec3::new(x, y, 0.0))))
    }

    /// The corners on screen of a card at `pose`, as `card_corners` gives them, and its depth.
    #[must_use]
    pub fn card_box(&self, pose: &Pose) -> ([Vec2; 4], f32) {
        (
            self.card_corners(pose.transform()),
            self.depth(pose.position),
        )
    }

    /// The point of the table (z = 0) that lies under `ndc`, a point of the screen in
    /// normalized device coordinates. The camera looks down at the table, so every point of the
    /// screen lies over one.
    #[must_use]
    pub fn on_table(&self, ndc: Vec2) -> Vec2 {
        let along = self.screen_pose(ndc, 1.0, Quat::IDENTITY, 1.0).position - self.eye;
        let point = self.eye - along * (self.eye.z / along.z);
        point.truncate()
    }

    /// The point `height` millimetres above the table that lies under `point`, a point on the
    /// window in logical points: where the line from the eye through `point` meets that plane.
    #[must_use]
    pub fn on_plane(&self, point: Vec2, height: f32) -> Vec3 {
        let size = self.window.size;
        let aspect = size.width / size.height;
        let ndc = Vec2::new(
            point.x / size.width * 2.0 - 1.0,
            1.0 - point.y / size.height * 2.0,
        );
        let direction = self.inverse_view.transform_vector3(Vec3::new(
            ndc.x * self.tan_half * aspect,
            ndc.y * self.tan_half,
            -1.0,
        ));
        self.eye + direction * ((height - self.eye.z) / direction.z)
    }

    /// The camera's right, up and backward directions in the table's space, each of unit
    /// length: across the screen, up the screen, and toward the camera.
    #[must_use]
    pub fn axes(&self) -> [Vec3; 3] {
        [
            self.inverse_view.x_axis.truncate().normalize(),
            self.inverse_view.y_axis.truncate().normalize(),
            self.inverse_view.z_axis.truncate().normalize(),
        ]
    }

    /// The transform that lays the unit quad on the flat layer's plane centred on `centre`, in
    /// logical points from the window's top left corner, `size.x` points long and `size.y` wide,
    /// its length along `way`, a direction on the screen of length 1: a quad of light along a
    /// line drawn on the screen (`light::arrows`). The rotation is built from the direction's
    /// two parts, with no angle, so that a ribbon of many quads takes no trigonometry.
    #[must_use]
    pub fn screen_transform(&self, centre: Vec2, size: Vec2, way: Vec2) -> Affine3A {
        let window = self.window.size;
        // One logical point on the layer's plane, in millimetres, the same across and down.
        let point = 2.0 * self.tan_half * LAYER_DEPTH / window.height;
        let ndc = Vec2::new(
            centre.x / window.width * 2.0 - 1.0,
            1.0 - centre.y / window.height * 2.0,
        );
        let half_height = self.tan_half * LAYER_DEPTH;
        let half_width = half_height * window.width / window.height;
        // The screen's y runs down and the camera's up, so a way down the screen points down
        // here.
        let (along, across) = (Vec3::new(way.x, -way.y, 0.0), Vec3::new(way.y, way.x, 0.0));
        let local = Affine3A::from_mat3_translation(
            Mat3::from_cols(along * (size.x * point), across * (size.y * point), Vec3::Z),
            Vec3::new(ndc.x * half_width, ndc.y * half_height, -LAYER_DEPTH),
        );
        Affine3A::from_mat4(self.inverse_view) * local
    }

    /// The flat plane the camera projects screen elements onto.
    #[must_use]
    pub fn screen_plane(&self) -> ScreenPlane {
        ScreenPlane::new(
            Affine3A::from_mat4(self.inverse_view),
            self.window,
            self.tan_half * LAYER_DEPTH,
            LAYER_DEPTH,
        )
    }

    /// The transform that lays the unit quad over `rect` on the flat drawing plane.
    #[must_use]
    pub fn layer_transform(&self, rect: Rect) -> Affine3A {
        self.screen_plane().layer_transform(rect)
    }
}
