//! The Enhanced tier's dedicated procedural VFX pipelines: main-pass premultiplied shading
//! (4x MSAA, depth-tested against the table) and additive bloom extraction (1x, no depth).
//!
//! Keeping procedural effects in their own shader module prevents them from adding dispatch
//! branches or wasted shadow-map / texture-atlas samples to Baseline's `shader.wgsl` or
//! Enhanced's lit `fs_lit`. New procedural Enhanced effects add a `kit_frame::frame::Vfx`
//! variant and a shading function here.

use crate::pipeline::{Pipelines, Shape, VERTEX_BUFFERS, pipeline_set};
use crate::runs::Pass;

use super::SAMPLES;
use super::bloom::ADD;

/// The procedural Enhanced VFX shader: entry points followed by each effect module.
/// Each effect is shaded by a function named `shade_<effect>`, which `shader_cost` measures.
pub const SHADER: &str = concat!(
    include_str!("vfx.wgsl"),
    include_str!("fx/common.wgsl"),
    include_str!("combat_fx.wgsl"),
    include_str!("line_fx.wgsl"),
    include_str!("fx/probe.wgsl"),
    include_str!("fx/counter.wgsl"),
    include_str!("fx/settle.wgsl"),
);

/// Pipelines for `Material::Vfx` records, all built once at startup.
pub(crate) struct Vfx {
    pipelines: Pipelines,
    /// Single-sample pipeline for VFX records in the flat layer (`Band::LayerCover`).
    layer: wgpu::RenderPipeline,
    bloom: wgpu::RenderPipeline,
}

impl Vfx {
    /// Builds the main-pass, flat-layer, and bloom-pass pipelines for targets of `format`,
    /// reading bind group 0 laid out as `records`.
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        records: &wgpu::BindGroupLayout,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("enhanced vfx"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let shape = Shape {
            shader: &shader,
            entry_points: ("vs_vfx", "fs_vfx"),
            format,
            samples: SAMPLES,
        };
        let pipelines = pipeline_set(device, &[records], &shape);
        let layer_shape = Shape {
            shader: &shader,
            entry_points: ("vs_vfx", "fs_vfx"),
            format,
            samples: 1,
        };
        let layer = pipeline_set(device, &[records], &layer_shape)
            .into_iter()
            .nth(Pass::Layer as usize)
            .expect("Pass::Layer pipeline exists");
        let bloom = bloom_pipeline(device, &shader, format, records);
        Self {
            pipelines,
            layer,
            bloom,
        }
    }

    /// The main-pass pipeline for `pass`.
    pub(crate) fn pipeline(&self, pass: Pass) -> &wgpu::RenderPipeline {
        &self.pipelines[pass as usize]
    }

    /// The single-sample flat-layer pipeline for `Band::LayerCover` VFX records.
    pub(crate) fn layer(&self) -> &wgpu::RenderPipeline {
        &self.layer
    }

    /// The additive one-sample pipeline that extracts VFX light into the bloom target.
    pub(crate) fn bloom(&self) -> &wgpu::RenderPipeline {
        &self.bloom
    }
}

fn bloom_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    records: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("vfx bloom"),
        bind_group_layouts: &[Some(records)],
        immediate_size: 0,
    });
    let additive = wgpu::BlendState {
        color: ADD,
        alpha: ADD,
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("vfx bloom"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_vfx"),
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &[("SHAKEN", 1.0)],
                ..Default::default()
            },
            buffers: &VERTEX_BUFFERS,
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_vfx_bloom"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(additive),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}
