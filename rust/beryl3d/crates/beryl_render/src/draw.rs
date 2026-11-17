//! Recording a frame's draw calls into a render pass, for both tiers: one instanced call per run
//! of records that share a pass, a mesh, a texture array and a clip (`runs`).

use std::ops::Range;

use beryl_scene::frame::Frame;

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
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);
        let mut draw_calls = 0;
        let mut state = None;
        for run in runs(self.frame) {
            if !wanted(run.pass) {
                continue;
            }
            let records = run.records.start..run.records.end.min(self.kept);
            if records.is_empty() {
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
                pass.set_pipeline(&pipelines[run.pass as usize]);
                pass.set_bind_group(0, bind_group, &[]);
                let [x, y, w, h] = scissor(self.frame, run.clip, self.width, self.height);
                pass.set_scissor_rect(x, y, w, h);
                state = Some((run.pass, run.array, run.clip));
            }
            // The run's records are bound from their offset, so every draw starts at
            // instance 0: OpenGL ES 3.0 has no first instance.
            let count = records.len() as u32;
            let bytes = u64::from(records.start) * RECORD..u64::from(records.end) * RECORD;
            pass.set_vertex_buffer(1, self.slot.instances.slice(bytes));
            pass.draw_indexed(indices.clone(), 0, 0..count);
            draw_calls += 1;
        }
        draw_calls
    }
}

/// The scissor rectangle for clip entry `clip` of `frame`, inside a target of `width` by
/// `height` pixels: the whole target for entry 0.
fn scissor(frame: &Frame, clip: u8, width: u32, height: u32) -> [u32; 4] {
    if clip == 0 {
        return [0, 0, width, height];
    }
    let rect = frame.clips[usize::from(clip)];
    let x = (rect.x.max(0.0) as u32).min(width);
    let y = (rect.y.max(0.0) as u32).min(height);
    let right = ((rect.x + rect.width).max(0.0).ceil() as u32).clamp(x, width);
    let bottom = ((rect.y + rect.height).max(0.0).ceil() as u32).clamp(y, height);
    [x, y, right - x, bottom - y]
}
