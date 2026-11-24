//! Resting and fitted poses for piles.

use glam::{Quat, Vec2, Vec3};
use kit_base::{data::Card, pose::Pose};

use super::Camera;

/// A pile advances one layer at a time in the caller's local coordinate system.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pile {
    pub step: Vec3,
}

impl Pile {
    /// A pile that grows vertically by one card layer at a time.
    #[must_use]
    pub fn vertical(card: Card) -> Self {
        Self {
            step: Vec3::Z * card.layer,
        }
    }

    /// Pose of the bottom card at a table anchor.
    #[must_use]
    pub fn bottom(card: Card, base: Vec2, scale: f32, tilt: f32) -> Pose {
        // Tilt around the near edge, keeping it at the height of a flat card.
        let lift = card.height / 2.0 * scale * tilt.sin();
        Pose {
            position: base.extend(card.layer / 2.0 + lift),
            rotation: Quat::from_rotation_x(tilt),
            scale,
            ..Pose::default()
        }
    }

    /// Fit the bottom of a pile so its top stays under `at` in logical points.
    /// The top card keeps the projected size it would have in a one-card pile.
    #[must_use]
    pub fn fit_top(self, camera: &Camera, at: Vec2, count: usize, nominal: f32, tilt: f32) -> Pose {
        let card = camera.data.card;
        let top_layer = self.offset(count.saturating_sub(1));
        // Along a screen ray, depth is proportional to vertical distance from the eye.
        // Keeping scale proportional to depth cancels the rise caused by the card's tilt.
        let scale = nominal * (1.0 - top_layer.z / (camera.eye.z - card.layer / 2.0));
        let mut bottom = Self::bottom(card, Vec2::ZERO, scale, tilt);
        bottom.position = camera.on_plane(at, bottom.position.z + top_layer.z) - top_layer;
        bottom
    }

    /// Keep the total displacement in the plane within `reach`. Depth advances
    /// independently, even when all the layers have the same position in the plane.
    #[must_use]
    pub fn limited(self, count: usize, reach: f32) -> Self {
        let distance = self.step.truncate().length();
        let gaps = count.saturating_sub(1).max(1) as f32;
        let across = if distance > 0.0 && distance * gaps > reach {
            self.step.truncate() * (reach / (distance * gaps))
        } else {
            self.step.truncate()
        };
        Self {
            step: across.extend(self.step.z),
        }
    }

    /// The offset from the first card's rest to this layer's rest.
    #[must_use]
    pub fn offset(self, layer: usize) -> Vec3 {
        self.step * layer as f32
    }

    /// Rest a layer above, below or beside the anchored card, with the same facing.
    #[must_use]
    pub fn pose(self, first: Pose, layer: usize) -> Pose {
        Pose {
            position: first.position + self.offset(layer),
            ..first
        }
    }
}
