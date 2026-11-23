//! Multi-node Catmull-Rom position splines with piecewise spherical rotation interpolation and
//! piecewise cubic Hermite timing curves.
//!
//! Used by card flights that follow an authored 3D path between endpoints: lifting off face down,
//! turning across edge-on in mid-flight, leaning into a 3D flourish, pausing at a hold point, and
//! settling into rest (or dropping down a flank and sweeping across the frame).
//!
//! `Spline` packs its control nodes and timing curve into 80 bytes so `Shape` and `Segment` stay
//! compact, `Copy`, and zero-allocation on animation frames.

use std::f32::consts::{FRAC_1_SQRT_2, PI};

use glam::{Quat, Vec3};

use super::Pose;

/// Maximum number of interior control nodes between `from` and `to` (supporting up to 6 total
/// spline nodes).
pub const MAX_SPLINE_INTERIOR: usize = 4;

/// Maximum number of keyframes in a piecewise cubic Hermite timing curve.
pub const MAX_HERMITE_KEYS: usize = 4;

/// Position quantization resolution in millimetres (`0.25 mm`, covering `+-8191 mm` from `to`).
const POS_SCALE: f32 = 4.0;
/// Seen-scale ratio quantization (`ratio = seen / to_seen`, covering `0..8x`).
const RATIO_SCALE: f32 = 8191.0;
/// Hermite slope quantization (covering `-8..+8`).
const SLOPE_SCALE: f32 = 4096.0;
/// Camera eye projection quantization (`0.5 mm`, covering `0..32767 mm`).
const EYE_SCALE: f32 = 2.0;

const FLAG_FLIPS: u8 = 1 << 4;
const FLAG_HALF_TURN: u8 = 1 << 5;
const FLAG_RETARGETED: u8 = 1 << 6;
const FLAG_BACK_Z_NEG: u8 = 1 << 7;

/// One keyframe of a piecewise cubic Hermite timing curve, mapping normalized step time `time`
/// in `[0, 1]` to normalized spline progress `value` in `[0, 1]`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HermiteKey {
    pub time: f32,
    pub value: f32,
    pub in_slope: f32,
    pub out_slope: f32,
}

/// A piecewise cubic Hermite timing curve over up to `MAX_HERMITE_KEYS` keyframes sorted by
/// ascending `time`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HermiteCurve {
    pub keys: [HermiteKey; MAX_HERMITE_KEYS],
    pub len: u8,
}

impl HermiteCurve {
    /// Evaluates the timing curve at normalized elapsed time `t` in `[0, 1]`, clamping outside
    /// the keyframe range to the first or last key's value.
    #[must_use]
    pub fn evaluate(&self, t: f32) -> f32 {
        let count = usize::from(self.len).min(MAX_HERMITE_KEYS);
        if count == 0 {
            return t.clamp(0.0, 1.0);
        }
        let keys = &self.keys[..count];
        if t <= keys[0].time || count == 1 {
            return keys[0].value;
        }
        let last = keys[count - 1];
        if t >= last.time {
            return last.value;
        }
        for pair in keys.windows(2) {
            let (k0, k1) = (pair[0], pair[1]);
            if t <= k1.time {
                let dt = k1.time - k0.time;
                if dt <= 1e-6 {
                    return k1.value;
                }
                let s = ((t - k0.time) / dt).clamp(0.0, 1.0);
                let s2 = s * s;
                let s3 = s2 * s;
                let h00 = 2.0 * s3 - 3.0 * s2 + 1.0;
                let h10 = s3 - 2.0 * s2 + s;
                let h01 = -2.0 * s3 + 3.0 * s2;
                let h11 = s3 - s2;
                return (h00 * k0.value
                    + h10 * dt * k0.out_slope
                    + h01 * k1.value
                    + h11 * dt * k1.in_slope)
                    .clamp(0.0, 1.0);
            }
        }
        last.value
    }
}

/// One interior control node of a 3D card spline, resolved into table space for the active camera.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SplineNode {
    /// Centre position in table millimetres.
    pub position: Vec3,
    /// Orientation in table space. When `flips` is true this is the full 3D mesh orientation
    /// (including any 180-degree face-down flip); otherwise it is `Pose::rotation` before `turn`.
    pub rotation: Quat,
    /// Apparent screen scale (`scale / depth`), preserving exact on-screen card size regardless
    /// of how far the card sits in front of the camera.
    pub seen_scale: f32,
}

/// Compact 12-byte representation of a 3- or 4-keyframe draw timing curve starting at `(0, 0)`
/// and ending at `value = 1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PackedHermite {
    t1: u16,
    v1: u16,
    out1: i16,
    t2: u16,
    v2: u16,
    t3: u16,
}

impl PackedHermite {
    fn pack(curve: HermiteCurve) -> (Self, bool) {
        let four = curve.len >= 4;
        let k1 = curve.keys[1];
        let k2 = curve.keys[2];
        let k3 = if four { curve.keys[3] } else { curve.keys[2] };
        (
            Self {
                t1: pack_unit(k1.time),
                v1: pack_unit(k1.value),
                out1: pack_scaled_i16(k1.out_slope, SLOPE_SCALE),
                t2: pack_unit(k2.time),
                v2: pack_unit(k2.value),
                t3: pack_unit(k3.time),
            },
            four,
        )
    }

    fn unpack(self, four: bool) -> HermiteCurve {
        let mut keys = [HermiteKey::default(); MAX_HERMITE_KEYS];
        keys[1] = HermiteKey {
            time: unpack_unit(self.t1),
            value: unpack_unit(self.v1),
            in_slope: 0.0,
            out_slope: f32::from(self.out1) / SLOPE_SCALE,
        };
        keys[2] = HermiteKey {
            time: unpack_unit(self.t2),
            value: if four { unpack_unit(self.v2) } else { 1.0 },
            in_slope: 0.0,
            out_slope: 0.0,
        };
        if four {
            keys[3] = HermiteKey {
                time: unpack_unit(self.t3),
                value: 1.0,
                in_slope: 0.0,
                out_slope: 0.0,
            };
        }
        HermiteCurve {
            keys,
            len: if four { 4 } else { 3 },
        }
    }
}

/// A resolved multi-node card spline between `Segment::from` (node `0`) and `Segment::to`
/// (node `count + 1`), packed into 80 bytes so `Segment` stays compact and `Copy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spline {
    /// Interior positions relative to `Segment::to.position`, quantized at `0.25 mm`.
    interior_pos: [[i16; 3]; MAX_SPLINE_INTERIOR],
    /// Interior rotations as smallest-three unit quaternions.
    interior_rot: [[i16; 3]; MAX_SPLINE_INTERIOR],
    /// Interior apparent screen scales as ratios of `to`'s apparent scale.
    interior_scale: [u16; MAX_SPLINE_INTERIOR],
    /// Packed 3- or 4-keyframe Hermite timing curve.
    ease: PackedHermite,
    /// Camera backward unit vector (`x, y` quantized; `z` reconstructed from unit length).
    camera_back_xy: [i16; 2],
    /// `camera.eye.dot(camera_back)` quantized at `0.5 mm`.
    eye_along_back: u16,
    /// When retargeted mid-flight by `Segment::through`, the eased progress `u0` at which
    /// `Segment::from` was replaced with the instantaneous pose.
    anchor_u: u16,
    /// Two-bit largest-component index for each of the four smallest-three quaternions.
    rot_largest: u8,
    /// Low 3 bits: interior node count (`1..=4`); bit 3: 4-key Hermite curve; high 4 bits: flags.
    flags: u8,
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "input is clamped to [0, 1] before scaling to u16::MAX"
)]
fn pack_unit(value: f32) -> u16 {
    (value.clamp(0.0, 1.0) * f32::from(u16::MAX)).round() as u16
}

fn unpack_unit(bits: u16) -> f32 {
    f32::from(bits) / f32::from(u16::MAX)
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "input is clamped to [i16::MIN, i16::MAX]"
)]
fn pack_scaled_i16(value: f32, scale: f32) -> i16 {
    (value * scale)
        .round()
        .clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "input is clamped to [0, u16::MAX]"
)]
fn pack_scaled_u16(value: f32, scale: f32) -> u16 {
    (value * scale).round().clamp(0.0, f32::from(u16::MAX)) as u16
}

/// Packs a unit quaternion into its 3 smallest components plus a 2-bit largest-component index.
fn pack_quat(quat: Quat) -> ([i16; 3], u8) {
    let q = quat.normalize().to_array();
    let mut largest = 0usize;
    let mut best = q[0].abs();
    for (i, &c) in q.iter().enumerate().skip(1) {
        if c.abs() > best {
            best = c.abs();
            largest = i;
        }
    }
    let sign = if q[largest] < 0.0 { -1.0 } else { 1.0 };
    let scale = f32::from(i16::MAX) / FRAC_1_SQRT_2;
    let mut packed = [0i16; 3];
    let mut out = 0;
    for (i, &c) in q.iter().enumerate() {
        if i != largest {
            packed[out] = pack_scaled_i16(c * sign, scale);
            out += 1;
        }
    }
    #[expect(clippy::cast_possible_truncation, reason = "largest is 0..4")]
    (packed, largest as u8)
}

/// Reconstructs a unit quaternion from smallest-three packing.
fn unpack_quat(packed: [i16; 3], largest: u8) -> Quat {
    let scale = FRAC_1_SQRT_2 / f32::from(i16::MAX);
    let mut comps = [0.0f32; 4];
    let mut sum_sq = 0.0f32;
    let mut src = 0;
    let largest_idx = usize::from(largest & 3);
    for (i, slot) in comps.iter_mut().enumerate() {
        if i != largest_idx {
            let v = f32::from(packed[src]) * scale;
            *slot = v;
            sum_sq += v * v;
            src += 1;
        }
    }
    comps[largest_idx] = (1.0 - sum_sq).max(0.0).sqrt();
    Quat::from_array(comps).normalize()
}

/// Evaluates one scalar uniform cubic Catmull-Rom segment between `c` and `f` with neighbour
/// points `p` (before `c`) and `ff` (after `f`) at local parameter `t` in `[0, 1]`.
fn catmull_rom_f32(p: f32, c: f32, f: f32, ff: f32, t: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * (2.0 * c
        + (-p + f) * t
        + (2.0 * p - 5.0 * c + 4.0 * f - ff) * t2
        + (-p + 3.0 * c - 3.0 * f + ff) * t3)
}

/// Evaluates one 3D uniform cubic Catmull-Rom segment between `c` and `f`.
fn catmull_rom_vec3(p: Vec3, c: Vec3, f: Vec3, ff: Vec3, t: f32) -> Vec3 {
    Vec3::new(
        catmull_rom_f32(p.x, c.x, f.x, ff.x, t),
        catmull_rom_f32(p.y, c.y, f.y, ff.y, t),
        catmull_rom_f32(p.z, c.z, f.z, ff.z, t),
    )
}

/// Full 3D orientation of `pose` as `Pose::transform` applies it.
fn effective_rotation(pose: &Pose) -> Quat {
    if pose.turn == 0.0 {
        pose.rotation
    } else {
        pose.rotation * Quat::from_rotation_x(PI * pose.turn)
    }
}

impl Spline {
    /// Packs `nodes` (up to `MAX_SPLINE_INTERIOR` interior nodes between `from` and `to`) and
    /// `ease` into a compact `Spline`.
    #[must_use]
    pub fn new(
        nodes: &[SplineNode],
        to: &Pose,
        ease: HermiteCurve,
        (flips, start_half_turn): (bool, bool),
        (camera_back, eye_along_back): (Vec3, f32),
    ) -> Self {
        let count = nodes.len().min(MAX_SPLINE_INTERIOR);
        let back = camera_back.normalize();
        let to_depth = (eye_along_back - to.position.dot(back)).max(1.0);
        let to_seen = (to.scale / to_depth).max(1e-6);

        let mut interior_pos = [[0i16; 3]; MAX_SPLINE_INTERIOR];
        let mut interior_rot = [[0i16; 3]; MAX_SPLINE_INTERIOR];
        let mut interior_scale = [0u16; MAX_SPLINE_INTERIOR];
        let mut rot_largest = 0u8;

        for (i, node) in nodes.iter().take(count).enumerate() {
            let delta = node.position - to.position;
            interior_pos[i] = [
                pack_scaled_i16(delta.x, POS_SCALE),
                pack_scaled_i16(delta.y, POS_SCALE),
                pack_scaled_i16(delta.z, POS_SCALE),
            ];
            let (packed_q, largest) = pack_quat(node.rotation);
            interior_rot[i] = packed_q;
            rot_largest |= (largest & 3) << (i * 2);
            interior_scale[i] = pack_scaled_u16(node.seen_scale / to_seen, RATIO_SCALE);
        }

        let (packed_ease, four_keys) = PackedHermite::pack(ease);
        #[expect(clippy::cast_possible_truncation, reason = "count is at most 4")]
        let mut flags = (count as u8) & 0x07;
        if four_keys {
            flags |= 1 << 3;
        }
        if flips {
            flags |= FLAG_FLIPS;
        }
        if start_half_turn {
            flags |= FLAG_HALF_TURN;
        }
        if back.z < 0.0 {
            flags |= FLAG_BACK_Z_NEG;
        }

        Self {
            interior_pos,
            interior_rot,
            interior_scale,
            ease: packed_ease,
            camera_back_xy: [
                pack_scaled_i16(back.x, f32::from(i16::MAX)),
                pack_scaled_i16(back.y, f32::from(i16::MAX)),
            ],
            eye_along_back: pack_scaled_u16(eye_along_back, EYE_SCALE),
            anchor_u: 0,
            rot_largest,
            flags,
        }
    }

    fn count(&self) -> usize {
        usize::from(self.flags & 0x07).min(MAX_SPLINE_INTERIOR)
    }

    fn four_keys(&self) -> bool {
        self.flags & (1 << 3) != 0
    }

    fn flips(&self) -> bool {
        self.flags & FLAG_FLIPS != 0
    }

    fn start_half_turn(&self) -> bool {
        self.flags & FLAG_HALF_TURN != 0
    }

    fn retargeted(&self) -> bool {
        self.flags & FLAG_RETARGETED != 0
    }

    fn camera_back(&self) -> Vec3 {
        let x = f32::from(self.camera_back_xy[0]) / f32::from(i16::MAX);
        let y = f32::from(self.camera_back_xy[1]) / f32::from(i16::MAX);
        let z_mag = (1.0 - x * x - y * y).max(0.0).sqrt();
        let z = if self.flags & FLAG_BACK_Z_NEG != 0 {
            -z_mag
        } else {
            z_mag
        };
        Vec3::new(x, y, z)
    }

    fn eye_along_back(&self) -> f32 {
        f32::from(self.eye_along_back) / EYE_SCALE
    }

    /// Distance of `position` in front of the camera eye, along the line of sight.
    fn depth(&self, position: Vec3) -> f32 {
        (self.eye_along_back() - position.dot(self.camera_back())).max(1.0)
    }

    /// Total number of nodes including endpoints (`2..=MAX_SPLINE_INTERIOR + 2`).
    fn total_nodes(&self) -> usize {
        self.count() + 2
    }

    /// Control node `index` in `0..self.total_nodes()`, clamping endpoints to `from` and `to`.
    fn node(&self, from: &Pose, to: &Pose, index: usize) -> SplineNode {
        let last = self.total_nodes() - 1;
        if index == 0 {
            let base_rot = if self.flips() {
                effective_rotation(from)
            } else {
                from.rotation
            };
            let rotation = if self.start_half_turn() && !self.retargeted() {
                base_rot * Quat::from_rotation_z(PI)
            } else {
                base_rot
            };
            SplineNode {
                position: from.position,
                rotation,
                seen_scale: from.scale / self.depth(from.position),
            }
        } else if index >= last {
            SplineNode {
                position: to.position,
                rotation: if self.flips() {
                    effective_rotation(to)
                } else {
                    to.rotation
                },
                seen_scale: to.scale / self.depth(to.position),
            }
        } else {
            let slot = index - 1;
            let raw_pos = self.interior_pos[slot];
            let position = to.position
                + Vec3::new(
                    f32::from(raw_pos[0]) / POS_SCALE,
                    f32::from(raw_pos[1]) / POS_SCALE,
                    f32::from(raw_pos[2]) / POS_SCALE,
                );
            let largest = (self.rot_largest >> (slot * 2)) & 3;
            let rotation = unpack_quat(self.interior_rot[slot], largest);
            let to_seen = to.scale / self.depth(to.position);
            let seen_scale = to_seen * (f32::from(self.interior_scale[slot]) / RATIO_SCALE);
            SplineNode {
                position,
                rotation,
                seen_scale,
            }
        }
    }

    /// Evaluates raw position, orientation, and apparent screen scale at eased progress `u`.
    fn raw_at(&self, from: &Pose, to: &Pose, u: f32) -> (Vec3, Quat, f32) {
        let total = self.total_nodes();
        let spans = total - 1;
        #[expect(
            clippy::cast_precision_loss,
            reason = "node count is at most 6, exact in f32"
        )]
        let scaled = u.clamp(0.0, 1.0) * spans as f32;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "scaled is clamped to [0, spans] where spans <= 5"
        )]
        let index = (scaled.floor() as usize).min(spans - 1);
        #[expect(
            clippy::cast_precision_loss,
            reason = "index is at most 4, exact in f32"
        )]
        let local_t = (scaled - index as f32).clamp(0.0, 1.0);

        let c = self.node(from, to, index);
        let f = self.node(from, to, index + 1);
        let p_pos = if index == 0 {
            c.position * 2.0 - f.position
        } else {
            self.node(from, to, index - 1).position
        };
        let ff_pos = if index + 2 >= total {
            f.position * 2.0 - c.position
        } else {
            self.node(from, to, index + 2).position
        };

        let position = catmull_rom_vec3(p_pos, c.position, f.position, ff_pos, local_t);
        let rotation = c.rotation.slerp(f.rotation, local_t);
        let seen_scale = c.seen_scale + (f.seen_scale - c.seen_scale) * local_t;
        (position, rotation, seen_scale)
    }

    /// Evaluates the full `Pose` at normalized elapsed time `elapsed` in `[0, 1]`.
    #[must_use]
    pub fn pose_at(&self, from: &Pose, to: &Pose, elapsed: f32) -> Pose {
        let curve = self.ease.unpack(self.four_keys());
        let u = curve.evaluate(elapsed);
        let (mut position, mut world_rot, mut seen_scale) = self.raw_at(from, to, u);

        // After a mid-flight resize (`Segment::through`), `from` holds the exact pose at
        // `anchor_u`. Blend smoothly from `from` (weight 1 at `anchor_u`) onto the spline
        // (weight 1 at `u = 1`) so there is no jump at the resize frame and arrival at `to` is
        // exact.
        if self.retargeted() {
            let u0 = unpack_unit(self.anchor_u);
            let denom = (1.0 - u0).max(1e-4);
            let keep = ((1.0 - u) / denom).clamp(0.0, 1.0);
            let anchor_rot = if self.flips() {
                effective_rotation(from)
            } else {
                from.rotation
            };
            let anchor_seen = from.scale / self.depth(from.position);
            position = position.lerp(from.position, keep);
            world_rot = world_rot.slerp(anchor_rot, keep);
            seen_scale = seen_scale + (anchor_seen - seen_scale) * keep;
        }

        let scale = seen_scale.max(1e-6) * self.depth(position);
        let camera_back = self.camera_back();
        let (rotation, turn) = if self.flips() {
            let front_faces_camera = (world_rot * Vec3::Z).dot(camera_back) >= 0.0;
            if front_faces_camera {
                (world_rot, 0.0)
            } else {
                (world_rot * Quat::from_rotation_x(-PI), 1.0)
            }
        } else {
            (world_rot, from.turn + (to.turn - from.turn) * u)
        };

        Pose {
            position,
            rotation,
            scale,
            opacity: from.opacity + (to.opacity - from.opacity) * u,
            turn,
            skew: from.skew + (to.skew - from.skew) * u,
            shear: from.shear + (to.shear - from.shear) * u,
        }
    }

    /// Retargets the spline through `target` at normalized time `elapsed` by recording `target`
    /// in `from` and anchoring the smooth blend at `u = ease(elapsed)`.
    pub fn through(&mut self, from: &mut Pose, elapsed: f32, target: Pose) {
        let curve = self.ease.unpack(self.four_keys());
        let u = curve.evaluate(elapsed);
        if 1.0 - u < 1e-4 {
            return;
        }
        *from = target;
        self.anchor_u = pack_unit(u);
        self.flags |= FLAG_RETARGETED;
    }

    /// Key normalized elapsed times in `(0, 1)` where interface checks should inspect this spline
    /// (for example: flip/flourish and hold plateau boundaries, or drop and sweep apex).
    #[must_use]
    pub fn moments(&self) -> [Option<f32>; 4] {
        if self.four_keys() {
            let hold_start = unpack_unit(self.ease.t1);
            let hold_end = unpack_unit(self.ease.t2);
            [
                Some(hold_start * 0.44),
                Some(hold_start * 0.74),
                Some(hold_start),
                Some(hold_end),
            ]
        } else {
            [Some(0.30), Some(0.60), None, None]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pose::Segment;

    fn local_ease() -> HermiteCurve {
        HermiteCurve {
            keys: [
                HermiteKey {
                    time: 0.0,
                    value: 0.0,
                    in_slope: 0.0,
                    out_slope: 0.0,
                },
                HermiteKey {
                    time: 0.57859,
                    value: 0.8,
                    in_slope: 0.0,
                    out_slope: 0.0,
                },
                HermiteKey {
                    time: 0.70068,
                    value: 0.8,
                    in_slope: 0.0,
                    out_slope: 0.0,
                },
                HermiteKey {
                    time: 0.94951,
                    value: 1.0,
                    in_slope: 0.0,
                    out_slope: 0.0,
                },
            ],
            len: 4,
        }
    }

    fn opponent_ease() -> HermiteCurve {
        HermiteCurve {
            keys: [
                HermiteKey {
                    time: 0.0,
                    value: 0.0,
                    in_slope: 0.0,
                    out_slope: 0.0,
                },
                HermiteKey {
                    time: 0.04998,
                    value: 0.0,
                    in_slope: 0.0,
                    out_slope: 1.03272,
                },
                HermiteKey {
                    time: 1.0,
                    value: 1.0,
                    in_slope: 0.0,
                    out_slope: 0.0,
                },
                HermiteKey::default(),
            ],
            len: 3,
        }
    }

    #[test]
    fn segment_stays_within_value_size_budget() {
        assert!(
            std::mem::size_of::<Segment>() <= 256,
            "Segment grew to {} bytes",
            std::mem::size_of::<Segment>()
        );
    }

    #[test]
    fn hermite_local_draw_holds_flat_plateau_and_settles_at_one() {
        let curve = local_ease();
        assert!((curve.evaluate(0.0) - 0.0).abs() < 1e-6);
        assert!((curve.evaluate(0.57859) - 0.8).abs() < 1e-6);
        assert!((curve.evaluate(0.64) - 0.8).abs() < 1e-6);
        assert!((curve.evaluate(0.70068) - 0.8).abs() < 1e-6);
        assert!((curve.evaluate(0.94951) - 1.0).abs() < 1e-6);
        assert!((curve.evaluate(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn hermite_opponent_draw_pauses_then_launches_monotonically() {
        let curve = opponent_ease();
        assert!((curve.evaluate(0.0) - 0.0).abs() < 1e-6);
        assert!((curve.evaluate(0.04998) - 0.0).abs() < 1e-6);
        let early = curve.evaluate(0.15);
        assert!(early > 0.1, "sharp launch after pause: {early}");
        assert!((curve.evaluate(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn spline_interpolates_control_nodes_and_preserves_transform_across_flip() {
        let from = Pose {
            position: Vec3::new(-300.0, -200.0, 0.0),
            rotation: Quat::IDENTITY,
            scale: 0.625,
            turn: 1.0,
            ..Pose::default()
        };
        let to = Pose {
            position: Vec3::new(0.0, -150.0, 900.0),
            rotation: Quat::IDENTITY,
            scale: 0.25,
            turn: 0.0,
            ..Pose::default()
        };
        let hold = Vec3::new(200.0, -50.0, 950.0);
        let nodes = [
            SplineNode {
                position: Vec3::new(-150.0, -80.0, 300.0),
                rotation: Quat::from_rotation_x(PI * 1.15),
                seen_scale: 0.00055,
            },
            SplineNode {
                position: Vec3::new(40.0, 10.0, 600.0),
                rotation: Quat::from_rotation_x(-0.6),
                seen_scale: 0.00065,
            },
            SplineNode {
                position: Vec3::new(140.0, 20.0, 850.0),
                rotation: Quat::from_euler(glam::EulerRot::YXZ, 0.3, -0.2, -0.35),
                seen_scale: 0.00080,
            },
            SplineNode {
                position: hold,
                rotation: Quat::IDENTITY,
                seen_scale: 0.00108,
            },
        ];
        let spline = Spline::new(&nodes, &to, local_ease(), (true, false), (Vec3::Z, 1200.0));

        // At hold plateau (u = 0.8), card rests at interior node 3 (node 4 of 6).
        let at_hold = spline.pose_at(&from, &to, 0.64);
        assert!(at_hold.position.distance(hold) < 0.5);
        assert!(at_hold.turn < 0.5);

        // Before edge-on it is face down (turn == 1.0); after edge-on it is face up (turn == 0.0).
        let early = spline.pose_at(&from, &to, 0.10);
        assert!(early.turn > 0.5);
        let late = spline.pose_at(&from, &to, 0.45);
        assert!(late.turn < 0.5);

        // Mid-flight resize continuity (`through`) hits target exactly and lands at `to`.
        let mid_t = 0.35;
        let current = spline.pose_at(&from, &to, mid_t);
        let shifted_to = Pose {
            position: Vec3::new(20.0, -140.0, 910.0),
            scale: 0.28,
            ..to
        };
        let mut adjusted = spline;
        let mut adjusted_from = from;
        adjusted.through(&mut adjusted_from, mid_t, current);
        let kept = adjusted.pose_at(&adjusted_from, &shifted_to, mid_t);
        assert!(kept.position.distance(current.position) < 1e-2);
        assert!((kept.scale - current.scale).abs() < 1e-3);
        let eff_kept = effective_rotation(&kept);
        let eff_cur = effective_rotation(&current);
        assert!(eff_kept.dot(eff_cur).abs() > 1.0 - 1e-4);

        let end = adjusted.pose_at(&adjusted_from, &shifted_to, 1.0);
        assert!(end.position.distance(shifted_to.position) < 1e-2);
        assert!((end.scale - shifted_to.scale).abs() < 1e-3);
    }
}
