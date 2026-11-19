//! The Baseline tier's pipelines, all created at startup (docs/beryl-3d.md, "The Baseline
//! tier"): one opaque pipeline that writes the depth buffer, and one blended pipeline in two
//! depth settings, tested against the depth buffer for the table's bands and untested for the
//! flat layer's. A pipeline's depth setting is fixed when it is made, so the flat layer needs
//! a pipeline of its own; its shader is the same.

use crate::mesh::Vertex;
use crate::runs::Pass;

/// The depth buffer's format.
pub const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// The size of one draw record, `beryl_scene::frame::Draw`.
pub const RECORD: u64 = 80;

const MESH_ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    0 => Float32x2,
    1 => Float32x2,
    2 => Float32x2,
    3 => Float32,
];

/// The draw record's fields, as `Draw` lays them out: three rows of the transform, `uv`,
/// `layer` with `material` and `flags`, `tint` and `params`.
const RECORD_ATTRIBUTES: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![
    4 => Float32x4,
    5 => Float32x4,
    6 => Float32x4,
    7 => Uint16x4,
    8 => Uint16x2,
    9 => Unorm8x4,
    10 => Float32x4,
];

/// The vertex buffers every pipeline that draws records reads: the mesh, then the records as
/// instances.
pub(crate) const VERTEX_BUFFERS: [Option<wgpu::VertexBufferLayout<'static>>; 2] = [
    Some(wgpu::VertexBufferLayout {
        array_stride: size_of::<Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &MESH_ATTRIBUTES,
    }),
    Some(wgpu::VertexBufferLayout {
        array_stride: RECORD,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &RECORD_ATTRIBUTES,
    }),
];

/// The layout of bind group 0: the uniforms, one texture array, its sampler and the frame's
/// paint table.
pub fn bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("beryl_render"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                // The vertex shader reads a sprite's page size.
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            // The frame's paint table: gradients' stops and clips' masks.
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: std::num::NonZeroU64::new(crate::renderer::PAINT_TABLE),
                },
                count: None,
            },
        ],
    })
}

/// The pipelines for the three passes, indexed by `Pass as usize`.
pub fn pipelines(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> [wgpu::RenderPipeline; 3] {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("beryl_render"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
    });
    let shape = Shape {
        shader: &shader,
        entry_points: ("vs", "fs"),
        format,
        samples: 1,
    };
    pipeline_set(device, &[layout], &shape)
}

/// What a set of pipelines draws with: a shader module and its two entry points, the target's
/// format and how many samples each pixel of the target holds.
pub struct Shape<'a> {
    pub shader: &'a wgpu::ShaderModule,
    /// The vertex and fragment entry points.
    pub entry_points: (&'static str, &'static str),
    pub format: wgpu::TextureFormat,
    pub samples: u32,
}

/// The pipelines of `shape` for the three passes, indexed by `Pass as usize`, with bind groups
/// laid out by `layouts` in order.
pub fn pipeline_set(
    device: &wgpu::Device,
    layouts: &[&wgpu::BindGroupLayout],
    shape: &Shape,
) -> [wgpu::RenderPipeline; 3] {
    let layouts: Vec<Option<&wgpu::BindGroupLayout>> = layouts.iter().copied().map(Some).collect();
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("beryl_render"),
        bind_group_layouts: &layouts,
        immediate_size: 0,
    });
    [Pass::Opaque, Pass::Table, Pass::Layer]
        .map(|pass| pipeline(device, &pipeline_layout, shape, pass))
}

fn pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shape: &Shape,
    pass: Pass,
) -> wgpu::RenderPipeline {
    let (blend, depth_write, depth_compare) = match pass {
        Pass::Opaque => (None, true, wgpu::CompareFunction::Less),
        Pass::Table => (
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            false,
            wgpu::CompareFunction::LessEqual,
        ),
        Pass::Layer => (
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            false,
            wgpu::CompareFunction::Always,
        ),
    };
    let shaken = [("SHAKEN", if pass == Pass::Layer { 0.0 } else { 1.0 })];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(match pass {
            Pass::Opaque => "opaque",
            Pass::Table => "blended",
            Pass::Layer => "flat layer",
        }),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shape.shader,
            entry_point: Some(shape.entry_points.0),
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &shaken,
                ..Default::default()
            },
            buffers: &VERTEX_BUFFERS,
        },
        fragment: Some(wgpu::FragmentState {
            module: shape.shader,
            entry_point: Some(shape.entry_points.1),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: shape.format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            // A card that turns over shows its back, so neither side is culled.
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH,
            depth_write_enabled: Some(depth_write),
            depth_compare: Some(depth_compare),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: shape.samples,
            ..Default::default()
        },
        multiview_mask: None,
        cache: None,
    })
}
