//! The draw list: 64-byte records in drawing order, as docs/beryl-3d.md ("The frame") specifies
//! for the Baseline tier. Opaque cards nearest first, then the table, all drawn with a depth
//! buffer; then the blended records farthest first: one shadow under each card above the table,
//! then cards that are fading. Every record uses the card mesh (the table and the shadows are the
//! mesh scaled), so each pass is one instanced draw call.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

use super::camera::Camera;
use super::motion::Pose;
use super::{LAYER_BACK, MAX_OBJECTS, Obj, Zone};

/// A card's size and corner radius in millimetres, as printed.
pub const CARD_W: f32 = 63.0;
pub const CARD_H: f32 = 88.0;
pub const CARD_RADIUS: f32 = 3.0;

pub const MATERIAL_TABLE: u8 = 0;
pub const MATERIAL_CARD: u8 = 1;
pub const MATERIAL_SHADOW: u8 = 2;

/// How large the table cloth is, in millimetres.
const TABLE_SIZE: f32 = 4000.0;
const TABLE_Z: f32 = -0.5;
/// A shadow quad is the card mesh scaled by this much, so its blurred edge fits inside it.
pub const SHADOW_SPREAD: f32 = 1.3;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Draw {
    /// A 3 by 4 transform into world space, row by row.
    pub model: [f32; 12],
    /// For a card, `uv[0]` is the layer of its back. For a shadow, `uv[0]` is its blur in
    /// hundredths of the card's width. The doc's record has no room for a card's second face, so
    /// the prototype borrows this field (see "What the prototype found" in the doc).
    pub uv: [u16; 4],
    pub layer: u16,
    pub material: u8,
    pub flags: u8,
    pub tint: [u8; 4],
}

const _: () = assert!(size_of::<Draw>() == 64);

/// What the core counts while it builds a frame, to predict the GPU's load.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStats {
    pub records: usize,
    pub opaque: usize,
    pub blended: usize,
    pub draw_calls: usize,
    /// Screen area of the opaque and of the blended records, in screens.
    pub opaque_area: f32,
    pub blended_area: f32,
}

pub struct DrawList {
    pub records: Vec<Draw>,
    pub naive: Vec<Draw>,
    pub stats: FrameStats,
    /// The lowest a shadow is drawn, above the cards lying on the table beneath it.
    pub shadow_height: f32,
    opaque: Vec<(f32, u16)>,
    blended: Vec<(f32, u16)>,
    shadows: Vec<(f32, u16)>,
    poses: Vec<Pose>,
}

fn rows(m: Mat4) -> [f32; 12] {
    let r = m.transpose().to_cols_array();
    [
        r[0], r[1], r[2], r[3], r[4], r[5], r[6], r[7], r[8], r[9], r[10], r[11],
    ]
}

impl DrawList {
    pub fn new() -> DrawList {
        let capacity = 2 * MAX_OBJECTS + 8;
        DrawList {
            records: Vec::with_capacity(capacity),
            naive: Vec::with_capacity(capacity),
            stats: FrameStats::default(),
            shadow_height: 8.0,
            opaque: Vec::with_capacity(MAX_OBJECTS),
            blended: Vec::with_capacity(MAX_OBJECTS),
            shadows: Vec::with_capacity(MAX_OBJECTS),
            poses: vec![Pose::HIDDEN; MAX_OBJECTS],
        }
    }

    fn collect(&mut self, objs: &[Obj], camera: &Camera, now: u64) {
        self.opaque.clear();
        self.blended.clear();
        self.shadows.clear();
        for (id, obj) in objs.iter().enumerate() {
            if obj.track.is_empty() {
                continue;
            }
            let pose = obj.track.pose_at(now);
            if pose.opacity < 0.004 {
                continue;
            }
            self.poses[id] = pose;
            let depth = camera.depth(pose.position);
            if pose.opacity >= 0.999 {
                self.opaque.push((depth, id as u16));
            } else {
                self.blended.push((depth, id as u16));
            }
            let raised = obj.track.moving(now) || matches!(obj.rec.zone, Zone::Hand | Zone::Stack);
            if raised && pose.position.z > 1.0 {
                self.shadows
                    .push((camera.depth(self.shadow_pose(&pose).position), id as u16));
            }
        }
        self.opaque.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        self.blended.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));
        self.shadows.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));
    }

    /// Builds the frame's records and counts. Allocates nothing.
    pub fn build(&mut self, objs: &[Obj], camera: &Camera, now: u64) {
        self.collect(objs, camera, now);
        self.records.clear();
        let mut stats = FrameStats::default();
        for i in 0..self.opaque.len() {
            let id = usize::from(self.opaque[i].1);
            let record = card(&self.poses[id], objs[id].rec.layer);
            stats.opaque_area += area(&record, camera);
            self.records.push(record);
        }
        let table = table();
        stats.opaque_area += area(&table, camera);
        self.records.push(table);
        stats.opaque = self.records.len();
        for i in 0..self.shadows.len() {
            let id = usize::from(self.shadows[i].1);
            let record = self.shadow(&self.poses[id]);
            stats.blended_area += area(&record, camera);
            self.records.push(record);
        }
        for i in 0..self.blended.len() {
            let id = usize::from(self.blended[i].1);
            let record = card(&self.poses[id], objs[id].rec.layer);
            stats.blended_area += area(&record, camera);
            self.records.push(record);
        }
        stats.records = self.records.len();
        stats.blended = stats.records - stats.opaque;
        stats.draw_calls = 1 + usize::from(stats.blended > 0);
        self.stats = stats;
    }

    /// Every record of the frame in one blended list, farthest first, the table first, for
    /// drawing with no depth buffer.
    pub fn build_naive(&mut self, objs: &[Obj], camera: &Camera, now: u64) {
        self.collect(objs, camera, now);
        self.naive.clear();
        self.naive.push(table());
        // Shadows lie on the table, under everything that is not, so they go first.
        for i in 0..self.shadows.len() {
            let id = usize::from(self.shadows[i].1);
            self.naive.push(self.shadow(&self.poses[id]));
        }
        self.blended.extend_from_slice(&self.opaque);
        self.blended.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));
        for i in 0..self.blended.len() {
            let id = usize::from(self.blended[i].1);
            self.naive.push(card(&self.poses[id], objs[id].rec.layer));
        }
    }

    fn shadow_pose(&self, pose: &Pose) -> Pose {
        let height = pose.position.z;
        let axis = pose.rotation * Vec3::X;
        let angle = axis.y.atan2(axis.x);
        Pose {
            position: Vec3::new(
                pose.position.x + height * 0.06,
                pose.position.y - height * 0.12,
                (height - 1.0).min(self.shadow_height),
            ),
            rotation: glam::Quat::from_rotation_z(angle),
            scale: pose.scale * SHADOW_SPREAD * (1.0 + height * 0.002).min(1.6),
            opacity: pose.opacity * (0.55 - height * 0.0012).clamp(0.15, 0.55),
        }
    }

    fn shadow(&self, pose: &Pose) -> Draw {
        let shadow = self.shadow_pose(pose);
        let blur = (8.0 + pose.position.z * 0.08).min(30.0);
        Draw {
            model: rows(shadow.matrix()),
            uv: [blur as u16, 0, 0, 0],
            layer: 0,
            material: MATERIAL_SHADOW,
            flags: 0,
            tint: [0, 0, 0, (shadow.opacity * 255.0) as u8],
        }
    }
}

fn card(pose: &Pose, layer: u16) -> Draw {
    Draw {
        model: rows(pose.matrix()),
        uv: [LAYER_BACK, 0, 0, 0],
        layer,
        material: MATERIAL_CARD,
        flags: 0,
        tint: [255, 255, 255, (pose.opacity.clamp(0.0, 1.0) * 255.0) as u8],
    }
}

fn table() -> Draw {
    let model = Mat4::from_translation(Vec3::new(0.0, 0.0, TABLE_Z))
        * Mat4::from_scale(Vec3::new(TABLE_SIZE / CARD_W, TABLE_SIZE / CARD_H, 1.0));
    Draw {
        model: rows(model),
        uv: [0; 4],
        layer: 0,
        material: MATERIAL_TABLE,
        flags: 0,
        tint: [255; 4],
    }
}

/// The record's screen area in screens: its rectangle's corners projected, clamped to the
/// screen, and measured with the shoelace formula. Clamping corners is not exact clipping, but
/// it is close for the convex quads of a card table.
fn area(record: &Draw, camera: &Camera) -> f32 {
    let m = &record.model;
    let half_w = CARD_W / 2.0;
    let half_h = CARD_H / 2.0;
    let mut points = [glam::Vec2::ZERO; 4];
    for (i, (x, y)) in [
        (-half_w, -half_h),
        (half_w, -half_h),
        (half_w, half_h),
        (-half_w, half_h),
    ]
    .into_iter()
    .enumerate()
    {
        let world = Vec3::new(
            m[0] * x + m[1] * y + m[3],
            m[4] * x + m[5] * y + m[7],
            m[8] * x + m[9] * y + m[11],
        );
        let clip = camera.clip(world);
        if clip.w <= 1e-3 {
            return 0.0;
        }
        points[i] = (clip.truncate().truncate() / clip.w)
            .clamp(glam::Vec2::splat(-1.0), glam::Vec2::splat(1.0));
    }
    let mut twice = 0.0;
    for i in 0..4 {
        let a = points[i];
        let b = points[(i + 1) % 4];
        twice += a.x * b.y - b.x * a.y;
    }
    twice.abs() / 2.0 / 4.0
}
