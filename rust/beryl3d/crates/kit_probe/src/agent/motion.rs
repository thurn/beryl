//! Continuity checks for pose tracks, independent of the objects they move.

use glam::Quat;
use kit_base::pose::{Pose, Segment};
use kit_space::motion::pose_at;

const SAME_MILLIMETRES: f32 = 0.01;
const SAME_FRACTION: f32 = 1e-3;

/// A visible discontinuity and the distance it covered in whole millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Jump {
    /// A segment starts at a different pose from the one reached by the track before it.
    Start(f32),
    /// A segment moves between two visible poses in zero time.
    ZeroDuration(f32),
}

/// Finds jumps in a track. The result includes the index of the segment that jumped.
/// `allows_instant` marks segments where an instant change is intentional.
pub fn jumps(
    segments: &[Segment],
    allows_instant: impl Fn(usize) -> bool,
) -> impl Iterator<Item = (usize, Jump)> {
    segments
        .iter()
        .enumerate()
        .flat_map(move |(index, segment)| {
            let start = (index != 0)
                .then(|| pose_at(&segments[..index], segment.start))
                .and_then(|before| visible_jump(before, segment.from))
                .map(Jump::Start);
            let instant = (segment.duration == 0 && !allows_instant(index))
                .then(|| visible_jump(segment.from, segment.to))
                .flatten()
                .map(Jump::ZeroDuration);
            [start, instant]
                .into_iter()
                .flatten()
                .map(move |jump| (index, jump))
        })
}

fn visible_jump(from: Pose, to: Pose) -> Option<f32> {
    (seen(from) && seen(to) && !same(from, to)).then(|| from.position.distance(to.position).round())
}

/// Whether two poses are the same to the eye.
#[must_use]
pub fn same(a: Pose, b: Pose) -> bool {
    if !seen(a) && !seen(b) {
        return true;
    }
    a.position.distance(b.position) < SAME_MILLIMETRES
        && turned(a.rotation, b.rotation) < SAME_FRACTION
        && (a.scale - b.scale).abs() < SAME_FRACTION
        && (a.opacity - b.opacity).abs() < SAME_FRACTION
        && (a.turn - b.turn).abs() < SAME_FRACTION
}

/// The angle between two rotations, in radians. `Quat::angle_between` takes the arc cosine of a
/// dot product near 1, which reads two equal rotations as about a thousandth of a radian apart
/// in `f32`; the sine of half the angle, the length of the vector part of the rotation from one
/// to the other, keeps its precision for small angles.
fn turned(a: Quat, b: Quat) -> f32 {
    let half = (b * a.inverse()).xyz().length().min(1.0);
    2.0 * half.asin()
}

/// Whether a pose can be seen at all.
#[must_use]
pub fn seen(pose: Pose) -> bool {
    pose.opacity > SAME_FRACTION
}
