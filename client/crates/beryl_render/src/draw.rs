//! Recording a frame's draw calls into a render pass, for both tiers: one instanced call per run
//! of records that share a pass, a mesh, a texture array and a clip (`runs`).

use std::ops::Range;

use glam::Vec2;
use kit_frame::frame::{Frame, Mesh};

use crate::count::{self, FragmentCounter};
use crate::pipeline::{Pipelines, RECORD};
use crate::runs::{Pass, Run, runs};
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
    pub(crate) ranges: &'a [Range<u32>; Mesh::COUNT],
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) timing: Option<&'a crate::gpu_timing::Work>,
}

impl Draws<'_> {
    /// Records the runs whose pass `wanted` accepts into `pass`, each with the pipeline of its
    /// pass from `pipelines`, and returns how many draw calls it recorded. Bind group 0 is set
    /// here; any other group the pipelines use is set by the caller.
    pub(crate) fn record(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        pipelines: &Pipelines,
        wanted: impl Fn(Pass) -> bool,
    ) -> u32 {
        self.record_part(
            pass,
            0..self.kept,
            |run| wanted(run.pass).then(|| &pipelines[run.pass as usize]),
            (self.width, self.height),
        )
    }

    /// Records the runs for which `pipeline` returns a pipeline into `pass`, and returns how
    /// many draw calls it recorded.
    pub(crate) fn record_with<'p>(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        pipeline: impl Fn(&Run) -> Option<&'p wgpu::RenderPipeline>,
    ) -> u32 {
        self.record_part(pass, 0..self.kept, pipeline, (self.width, self.height))
    }

    /// Records the frame's records in `records` into `pass`, a pass over a target `target`
    /// pixels large, and returns how many draw calls it recorded. Each run is drawn with the
    /// pipeline `pipeline` gives for it, and skipped where it gives none. A run that straddles
    /// the range is cut to it. Clip rectangles, which are in the frame's pixels, are scaled to
    /// the target.
    pub(crate) fn record_part<'p>(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        records: Range<u32>,
        pipeline: impl Fn(&Run) -> Option<&'p wgpu::RenderPipeline>,
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
            let Some(drawn_with) = pipeline(&run) else {
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
            if state.is_none_or(|(pass, _, _, _)| pass != run.pass) {
                // The moving passes use the near half of the depth buffer. They cover the
                // resting table and still depth-test correctly against each other.
                let (near, far) = match run.pass {
                    Pass::Opaque | Pass::Table => (0.5, 1.0),
                    Pass::MovingOpaque | Pass::Moving => (0.0, 0.5),
                    Pass::Layer | Pass::LayerOpaque => (0.0, 1.0),
                };
                pass.set_viewport(0.0, 0.0, target.0 as f32, target.1 as f32, near, far);
            }
            if state != Some((run.pass, run.array, run.clip, run.vfx)) {
                pass.set_pipeline(drawn_with);
                pass.set_bind_group(0, bind_group, &[]);
                let [x, y, w, h] = scissor(self.frame, run.clip, target, scale);
                pass.set_scissor_rect(x, y, w, h);
                state = Some((run.pass, run.array, run.clip, run.vfx));
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

/// Records the Baseline tier's one pass over every record of `draws` into `encoder`, drawing
/// into `view` with the one-sample `depth` buffer, and returns how many draw calls it took.
/// `counter`, when present, counts the fragments the pass shades.
pub(crate) fn baseline_pass(
    encoder: &mut wgpu::CommandEncoder,
    draws: &Draws,
    view: &wgpu::TextureView,
    depth: &wgpu::TextureView,
    pipelines: &Pipelines,
    counter: Option<&FragmentCounter>,
) -> u32 {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("frame"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Discard,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: draws.timing.and_then(crate::gpu_timing::Work::timestamps),
        occlusion_query_set: None,
        multiview_mask: None,
    });
    count::begin(counter, &mut pass);
    let draw_calls = draws.record(&mut pass, pipelines, |_| true);
    count::end(counter, &mut pass);
    draw_calls
}

/// Records the flat layer's records from `at` on, those drawn over a backdrop filter, into
/// `view`, over what it holds, and returns how many draw calls it took.
pub(crate) fn over_pass(
    encoder: &mut wgpu::CommandEncoder,
    draws: &Draws,
    at: u32,
    view: &wgpu::TextureView,
    depth: &wgpu::TextureView,
    pipelines: &Pipelines,
    counter: Option<&FragmentCounter>,
) -> u32 {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("over the backdrop"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Discard,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    count::begin(counter, &mut pass);
    let calls = draws.record_part(
        &mut pass,
        at..draws.kept,
        |run| Some(&pipelines[run.pass as usize]),
        (draws.width, draws.height),
    );
    count::end(counter, &mut pass);
    calls
}
