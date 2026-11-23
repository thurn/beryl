//! Multi-node Catmull-Rom position splines with piecewise spherical rotation interpolation and
//! piecewise cubic Hermite timing curves.
//!
//! Used by card flights that follow an authored 3D path between endpoints: lifting off face down,
//! turning across edge-on in mid-flight, leaning into a 3D flourish, pausing at a hold point, and
//! settling into rest (or dropping down a flank and sweeping across the frame).
//!
//! Positions are interpolated in homogeneous camera-ray space `(ray * w, w)` weighted by each
//! node's reference frustum depth ratio `w = 1 / rel_scale`, then placed along the active camera
//! depth curve. That reproduces a reference camera's exact screen-space trajectory and velocity
//! profile even when the endpoints sit at very different depths in the local camera frustum.
//!
//! `Spline` packs its control nodes and timing curve so `Segment` fits in 256 bytes and stays
//! `Copy`, with no allocations on animation frames.

use std::f32::consts::{FRAC_1_SQRT_2, PI};

use glam::{Quat, Vec3, Vec4};

use super::Pose;

/// Maximum number of interior control nodes between `from` and `to` (supporting up to 6 total
/// spline nodes).
pub const MAX_SPLINE_INTERIOR: usize = 4;

/// Maximum number of keyframes in a piecewise cubic Hermite timing curve.
pub const MAX_HERMITE_KEYS: usize = 4;

/// Camera eye quantization resolution (`0.5 mm`, covering `+-16383 mm`).
const EYE_SCALE: f32 = 2.0;
/// Unit-depth camera ray XY quantization (covering `+-2.0` at `0.000061` precision).
const RAY_SCALE: f32 = 16384.0;
/// Node depth quantization (`0.25 mm`, covering `0..16383 mm`).
const DEPTH_SCALE: f32 = 4.0;
/// Seen-scale ratio quantization (`ratio = seen / to_seen`, covering `0..8x`).
const RATIO_SCALE: f32 = 8191.0;
/// Hermite slope quantization (covering `-8..+8`).
const SLOPE_SCALE: f32 = 4096.0;
/// Six-bit frustum depth weight range `[WEIGHT_MIN, WEIGHT_MIN + WEIGHT_SPAN]`.
const WEIGHT_MIN: f32 = 0.5;
const WEIGHT_SPAN: f32 = 1.0;

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
    /// Reference frustum depth ratio (`1 / rel_scale`) used as the homogeneous weight for
    /// perspective-matched Catmull-Rom ray interpolation.
    pub weight: f32,
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

/// Unpacked node in homogeneous ray space used during spline evaluation.
#[derive(Clone, Copy)]
struct EvalNode {
    ray: Vec3,
    depth: f32,
    weight: f32,
    rotation: Quat,
    seen_scale: f32,
}

/// A resolved multi-node card spline between `Segment::from` (node `0`) and `Segment::to`
/// (node `count + 1`), packed so `Segment` stays compact and `Copy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spline {
    /// Camera eye position in millimetres, quantized at `0.5 mm`.
    eye: [i16; 3],
    /// Interior unit-depth camera ray `(x, y)` coordinates (`z` reconstructed from `ray · back = -1`).
    interior_ray_xy: [[i16; 2]; MAX_SPLINE_INTERIOR],
    /// Interior depths in front of `eye`, quantized at `0.25 mm`.
    interior_depth: [u16; MAX_SPLINE_INTERIOR],
    /// Interior rotations as smallest-three unit quaternions.
    interior_rot: [[i16; 3]; MAX_SPLINE_INTERIOR],
    /// Interior apparent screen scales as ratios of `to`'s apparent scale.
    interior_scale: [u16; MAX_SPLINE_INTERIOR],
    /// Packed 3- or 4-keyframe Hermite timing curve.
    ease: PackedHermite,
    /// Camera backward unit vector (`x, y` quantized; `z` reconstructed from unit length).
    camera_back_xy: [i16; 2],
    /// Exact `f32` bits of the elapsed fraction when `Segment::through` captured the pose.
    anchor_elapsed_bits: u32,
    /// Five 6-bit homogeneous frustum depth weights (`from` followed by up to 4 interior nodes).
    weights: [u8; 4],
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

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "weight is clamped to [0, 63]"
)]
fn pack_weights(values: [f32; 5]) -> [u8; 4] {
    let mut bits = 0u32;
    for (i, &w) in values.iter().enumerate() {
        let q = (((w - WEIGHT_MIN) / WEIGHT_SPAN).clamp(0.0, 1.0) * 63.0).round() as u32;
        bits |= (q & 0x3f) << (i * 6);
    }
    bits.to_le_bytes()
}

fn unpack_weight(bytes: [u8; 4], index: usize) -> f32 {
    let bits = u32::from_le_bytes(bytes);
    let q = (bits >> (index * 6)) & 0x3f;
    #[expect(clippy::cast_precision_loss, reason = "q is 0..64")]
    let share = q as f32 / 63.0;
    WEIGHT_MIN + share * WEIGHT_SPAN
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

/// Evaluates one 4D homogeneous uniform cubic Catmull-Rom segment between `c` and `f`.
fn catmull_rom_vec4(p: Vec4, c: Vec4, f: Vec4, ff: Vec4, t: f32) -> Vec4 {
    Vec4::new(
        catmull_rom_f32(p.x, c.x, f.x, ff.x, t),
        catmull_rom_f32(p.y, c.y, f.y, ff.y, t),
        catmull_rom_f32(p.z, c.z, f.z, ff.z, t),
        catmull_rom_f32(p.w, c.w, f.w, ff.w, t),
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
        from_weight: f32,
        to: &Pose,
        ease: HermiteCurve,
        (flips, start_half_turn): (bool, bool),
        (camera_eye, camera_back): (Vec3, Vec3),
    ) -> Self {
        let count = nodes.len().min(MAX_SPLINE_INTERIOR);
        let back = camera_back.normalize();
        let eye_packed = [
            pack_scaled_i16(camera_eye.x, EYE_SCALE),
            pack_scaled_i16(camera_eye.y, EYE_SCALE),
            pack_scaled_i16(camera_eye.z, EYE_SCALE),
        ];
        let eye = Vec3::new(
            f32::from(eye_packed[0]) / EYE_SCALE,
            f32::from(eye_packed[1]) / EYE_SCALE,
            f32::from(eye_packed[2]) / EYE_SCALE,
        );
        let to_depth = (eye - to.position).dot(back).max(1.0);
        let to_seen = (to.scale / to_depth).max(1e-6);

        let mut interior_ray_xy = [[0i16; 2]; MAX_SPLINE_INTERIOR];
        let mut interior_depth = [0u16; MAX_SPLINE_INTERIOR];
        let mut interior_rot = [[0i16; 3]; MAX_SPLINE_INTERIOR];
        let mut interior_scale = [0u16; MAX_SPLINE_INTERIOR];
        let mut weight_list = [1.0f32; 5];
        weight_list[0] = from_weight;
        let mut rot_largest = 0u8;

        for (i, node) in nodes.iter().take(count).enumerate() {
            let depth = (eye - node.position).dot(back).max(1.0);
            let ray = (node.position - eye) / depth;
            interior_ray_xy[i] = [
                pack_scaled_i16(ray.x, RAY_SCALE),
                pack_scaled_i16(ray.y, RAY_SCALE),
            ];
            interior_depth[i] = pack_scaled_u16(depth, DEPTH_SCALE);
            let (packed_q, largest) = pack_quat(node.rotation);
            interior_rot[i] = packed_q;
            rot_largest |= (largest & 3) << (i * 2);
            interior_scale[i] = pack_scaled_u16(node.seen_scale / to_seen, RATIO_SCALE);
            weight_list[i + 1] = if node.weight > 0.0 { node.weight } else { 1.0 };
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
            eye: eye_packed,
            interior_ray_xy,
            interior_depth,
            interior_rot,
            interior_scale,
            ease: packed_ease,
            camera_back_xy: [
                pack_scaled_i16(back.x, f32::from(i16::MAX)),
                pack_scaled_i16(back.y, f32::from(i16::MAX)),
            ],
            anchor_elapsed_bits: 0,
            weights: pack_weights(weight_list),
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

    fn eye(&self) -> Vec3 {
        Vec3::new(
            f32::from(self.eye[0]) / EYE_SCALE,
            f32::from(self.eye[1]) / EYE_SCALE,
            f32::from(self.eye[2]) / EYE_SCALE,
        )
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

    /// Distance of `position` in front of the camera eye, along the line of sight.
    fn depth(position: Vec3, eye: Vec3, back: Vec3) -> f32 {
        (eye - position).dot(back).max(1.0)
    }

    /// Total number of nodes including endpoints (`2..=MAX_SPLINE_INTERIOR + 2`).
    fn total_nodes(&self) -> usize {
        self.count() + 2
    }

    /// Control node `index` in `0..self.total_nodes()`, clamping endpoints to `from` and `to`.
    fn node(&self, from: &Pose, to: &Pose, index: usize, eye: Vec3, back: Vec3) -> EvalNode {
        let last = self.total_nodes() - 1;
        if index == 0 {
            let depth = Self::depth(from.position, eye, back);
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
            EvalNode {
                ray: (from.position - eye) / depth,
                depth,
                weight: unpack_weight(self.weights, 0),
                rotation,
                seen_scale: from.scale / depth,
            }
        } else if index >= last {
            let depth = Self::depth(to.position, eye, back);
            EvalNode {
                ray: (to.position - eye) / depth,
                depth,
                weight: 1.0,
                rotation: if self.flips() {
                    effective_rotation(to)
                } else {
                    to.rotation
                },
                seen_scale: to.scale / depth,
            }
        } else {
            let slot = index - 1;
            let rx = f32::from(self.interior_ray_xy[slot][0]) / RAY_SCALE;
            let ry = f32::from(self.interior_ray_xy[slot][1]) / RAY_SCALE;
            let bz = if back.z.abs() > 1e-4 {
                back.z
            } else {
                1.0f32.copysign(back.z)
            };
            let rz = (-1.0 - rx * back.x - ry * back.y) / bz;
            let depth = (f32::from(self.interior_depth[slot]) / DEPTH_SCALE).max(40.0);
            let largest = (self.rot_largest >> (slot * 2)) & 3;
            let rotation = unpack_quat(self.interior_rot[slot], largest);
            let to_depth = Self::depth(to.position, eye, back);
            let to_seen = to.scale / to_depth;
            let seen_scale = to_seen * (f32::from(self.interior_scale[slot]) / RATIO_SCALE);
            EvalNode {
                ray: Vec3::new(rx, ry, rz),
                depth,
                weight: unpack_weight(self.weights, index),
                rotation,
                seen_scale,
            }
        }
    }

    /// Evaluates raw position, orientation, and apparent screen scale at eased progress `u`.
    fn raw_at(&self, from: &Pose, to: &Pose, u: f32, eye: Vec3, back: Vec3) -> (Vec3, Quat, f32) {
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

        let c = self.node(from, to, index, eye, back);
        let f = self.node(from, to, index + 1, eye, back);
        let hom = |n: EvalNode| (n.ray * n.weight).extend(n.weight);
        let (hc, hf) = (hom(c), hom(f));
        let (hp, dp) = if index == 0 {
            (hc * 2.0 - hf, c.depth * 2.0 - f.depth)
        } else {
            let prev = self.node(from, to, index - 1, eye, back);
            (hom(prev), prev.depth)
        };
        let (hff, dff) = if index + 2 >= total {
            (hf * 2.0 - hc, f.depth * 2.0 - c.depth)
        } else {
            let next = self.node(from, to, index + 2, eye, back);
            (hom(next), next.depth)
        };

        let h_u = catmull_rom_vec4(hp, hc, hf, hff, local_t);
        let ray = h_u.truncate() / h_u.w.max(1e-4);
        let depth = catmull_rom_f32(dp, c.depth, f.depth, dff, local_t).max(40.0);
        let position = eye + ray * depth;
        let rotation = c.rotation.slerp(f.rotation, local_t);
        let seen_scale = c.seen_scale + (f.seen_scale - c.seen_scale) * local_t;
        (position, rotation, seen_scale)
    }

    /// Evaluates the full `Pose` at normalized elapsed time `elapsed` in `[0, 1]`.
    #[must_use]
    pub fn pose_at(&self, from: &Pose, to: &Pose, elapsed: f32) -> Pose {
        let curve = self.ease.unpack(self.four_keys());
        let u = curve.evaluate(elapsed);
        let retargeted = self.retargeted();
        let progress = if retargeted {
            let anchor_elapsed = f32::from_bits(self.anchor_elapsed_bits);
            if elapsed <= anchor_elapsed {
                return *from;
            }
            let anchor_u = curve.evaluate(anchor_elapsed);
            let remaining = 1.0 - anchor_u;
            // The curve can already be at its destination before the segment ends. Use the
            // remaining time in that case so the card can still move to its new destination.
            let progress = if remaining < 1e-4 {
                (elapsed - anchor_elapsed) / (1.0 - anchor_elapsed)
            } else {
                (u - anchor_u) / remaining
            };
            if progress <= 0.0 {
                return *from;
            }
            if progress >= 1.0 {
                return *to;
            }
            progress
        } else {
            u
        };
        let eye = self.eye();
        let camera_back = self.camera_back();
        let (mut position, mut world_rot, mut seen_scale) =
            self.raw_at(from, to, u, eye, camera_back);

        // After a resize, blend from the captured pose onto the spline over the remaining flight.
        if retargeted {
            let anchor_rot = if self.flips() {
                effective_rotation(from)
            } else {
                from.rotation
            };
            let anchor_seen = from.scale / Self::depth(from.position, eye, camera_back);
            position = from.position.lerp(position, progress);
            world_rot = anchor_rot.slerp(world_rot, progress);
            seen_scale = anchor_seen + (seen_scale - anchor_seen) * progress;
        }

        let scale = seen_scale.max(1e-6) * Self::depth(position, eye, camera_back);
        let (rotation, turn) = if self.flips() {
            let front_faces_camera = (world_rot * Vec3::Z).dot(camera_back) >= 0.0;
            if front_faces_camera {
                (world_rot, 0.0)
            } else {
                (world_rot * Quat::from_rotation_x(-PI), 1.0)
            }
        } else {
            (world_rot, from.turn + (to.turn - from.turn) * progress)
        };

        Pose {
            position,
            rotation,
            scale,
            opacity: from.opacity + (to.opacity - from.opacity) * progress,
            turn,
            skew: from.skew + (to.skew - from.skew) * progress,
            shear: from.shear + (to.shear - from.shear) * progress,
        }
    }

    /// Retargets the spline from `target` by storing the pose and its normalized time `elapsed`.
    pub fn through(&mut self, from: &mut Pose, elapsed: f32, target: Pose) {
        *from = target;
        self.anchor_elapsed_bits = elapsed.to_bits();
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
#[path = "spline_tests.rs"]
mod tests;
