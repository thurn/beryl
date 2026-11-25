//! Moving a spline's start once it is made, for a card that begins its flight somewhere other
//! than where the verb that made the spline had it: a hand card the player dragged out and let
//! go before its cast flies it to the stack (`Segment::start_at`).

use glam::Vec3;

use super::{DEPTH_SCALE, RATIO_SCALE, RAY_SCALE, Spline, pack_scaled_i16, pack_scaled_u16};
use crate::pose::Pose;

impl Spline {
    /// Moves the path so that it starts at `now` instead of `was`, still ending at `to`. Each
    /// interior node keeps its place relative to the two ends: it moves by the start's move,
    /// shrunk by how far toward `to` the node lies along the straight way, as the camera sees
    /// it. A node a fifth of the way from `was` to `to` moves four fifths as far as the start
    /// does, so the path leaves `now` the way it would have left `was`, and none of it bends
    /// back toward `was`. Its apparent size shifts with the start's apparent size the same way.
    /// A spline already retargeted mid-flight (`through`) holds its anchor and is left alone.
    pub fn moved_start(&mut self, was: &Pose, now: &Pose, to: &Pose) {
        if self.retargeted() {
            return;
        }
        let (eye, back) = (self.eye(), self.camera_back());
        let seen = |pose: &Pose| {
            let depth = Self::depth(pose.position, eye, back);
            ((pose.position - eye) / depth, depth, pose.scale / depth)
        };
        let (was_ray, was_depth, was_seen) = seen(was);
        let (now_ray, now_depth, now_seen) = seen(now);
        let (to_ray, _, to_seen) = seen(to);
        let way = to_ray - was_ray;
        for slot in 0..self.count() {
            let node = self.node(was, to, slot + 1, eye, back);
            let toward = if way.length_squared() > f32::EPSILON {
                ((node.ray - was_ray).dot(way) / way.length_squared()).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let keep = 1.0 - toward;
            // Every ray from the eye at unit depth lies in the plane `ray · back = -1`, and so
            // does this one, so `node` can rebuild its `z` from the two stored components.
            let ray: Vec3 = node.ray + (now_ray - was_ray) * keep;
            self.interior_ray_xy[slot] = [
                pack_scaled_i16(ray.x, RAY_SCALE),
                pack_scaled_i16(ray.y, RAY_SCALE),
            ];
            let depth = node.depth + (now_depth - was_depth) * keep;
            self.interior_depth[slot] = pack_scaled_u16(depth, DEPTH_SCALE);
            let seen_scale = node.seen_scale + (now_seen - was_seen) * keep;
            self.interior_scale[slot] =
                pack_scaled_u16(seen_scale / to_seen.max(1e-6), RATIO_SCALE);
        }
    }
}
