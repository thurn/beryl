//! Recording a frame's draw calls into a render pass, for both tiers: one instanced call per run
//! of records that share a pass, a mesh, a texture array and a clip (`runs`).

use std::ops::Range;

use beryl_scene::frame::Frame;
use glam::Vec2;

use crate::pipeline::RECORD;
use crate::runs::{Pass, runs};
use crate::slots::Slot;

/// What every pass of one frame draws from: the frame, the buffers its records were written
/// into and the meshes, in a target `width` by `height` physical pixels.
pub(crate) struct Draws<'a> {
    pub(crate) frame: &'a Frame,
    /// How many of the frame's records the instance buffer holds.
    pub(crate) kept: u32,
    pub(crate) slot: &'a Slot,
    pub(crate) vertices: &'a wgpu::Buffer,
    pub(crate) indices: &'a wgpu::Buffer,
    /// The index range of each mesh, by `Mesh` value.
    pub(crate) ranges: &'a [Range<u32>; 4],
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl Draws<'_> {
    /// Records the runs whose pass `wanted` accepts into `pass`, each with the pipeline of its
    /// pass from `pipelines`, and returns how many draw calls it recorded. Bind group 0 is set
    /// here; any other group the pipelines use is set by the caller.
    pub(crate) fn record(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        pipelines: &[wgpu::RenderPipeline; 3],
        wanted: impl Fn(Pass) -> bool,
    ) -> u32 {
        self.record_part(
            pass,
            0..self.kept,
            |run| wanted(run).then(|| &pipelines[run as usize]),
            (self.width, self.height),
        )
    }

    /// Records the frame's records in `records` into `pass`, a pass over a target `target`
    /// pixels large, and returns how many draw calls it recorded. Each run is drawn with the
    /// pipeline `pipeline` gives for its pass, and skipped where it gives none. A run that
    /// straddles the range is cut to it. Clip rectangles, which are in the frame's pixels, are
    /// scaled to the target.
    pub(crate) fn record_part<'p>(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        records: Range<u32>,
        pipeline: impl Fn(Pass) -> Option<&'p wgpu::RenderPipeline>,
        target: (u32, u32),
    ) -> u32 {
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);
        let scale = Vec2::new(
            target.0 as f32 / self.width.max(1) as f32,
            target.1 as f32 / self.height.max(1) as f32,
        );
        let end = records.end.min(self.kept);
        let mut draw_calls = 0;
        let mut state = None;
        for run in runs(self.frame) {
            let Some(drawn_with) = pipeline(run.pass) else {
                continue;
            };
            let part = run.records.start.max(records.start)..run.records.end.min(end);
            if part.is_empty() {
                continue;
            }
            let Some(indices) = self.ranges.get(usize::from(run.mesh)) else {
                debug_assert!(false, "a record names mesh {}", run.mesh);
                continue;
            };
            let Some(bind_group) = self.slot.bind_groups.get(usize::from(run.array)) else {
                debug_assert!(false, "a record names texture array {}", run.array);
                continue;
            };
            if state != Some((run.pass, run.array, run.clip)) {
                pass.set_pipeline(drawn_with);
                pass.set_bind_group(0, bind_group, &[]);
                let [x, y, w, h] = scissor(self.frame, run.clip, target, scale);
                pass.set_scissor_rect(x, y, w, h);
                state = Some((run.pass, run.array, run.clip));
            }
            // The run's records are bound from their offset, so every draw starts at
            // instance 0: OpenGL ES 3.0 has no first instance.
            let count = part.len() as u32;
            let bytes = u64::from(part.start) * RECORD..u64::from(part.end) * RECORD;
            pass.set_vertex_buffer(1, self.slot.instances.slice(bytes));
            pass.draw_indexed(indices.clone(), 0, 0..count);
            draw_calls += 1;
        }
        draw_calls
    }
}

/// The scissor rectangle for clip entry `clip` of `frame`, inside a target `width` by `height`
/// pixels large whose pixels are `scale` of the frame's: the whole target for entry 0.
fn scissor(frame: &Frame, clip: u8, (width, height): (u32, u32), scale: Vec2) -> [u32; 4] {
    if clip == 0 {
        return [0, 0, width, height];
    }
    let rect = frame.clips[usize::from(clip)];
    let (left, top) = (rect.x * scale.x, rect.y * scale.y);
    let (far_right, far_bottom) = (
        (rect.x + rect.width) * scale.x,
        (rect.y + rect.height) * scale.y,
    );
    let x = (left.max(0.0) as u32).min(width);
    let y = (top.max(0.0) as u32).min(height);
    let right = (far_right.max(0.0).ceil() as u32).clamp(x, width);
    let bottom = (far_bottom.max(0.0).ceil() as u32).clamp(y, height);
    [x, y, right - x, bottom - y]
}
