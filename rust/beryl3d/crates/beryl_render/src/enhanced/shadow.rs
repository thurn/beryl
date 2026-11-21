//! The Enhanced tier's shadow map (docs/beryl-3d.md, "The Enhanced tier": "A shadow map"),
//! sampled by `lit_by_key` in `shadow.wgsl`.
//!
//! Every card before the flat layer casts a shadow of its true shape from one directional key
//! light, on the table and on the cards below it. The light points the way the Baseline tier's
//! contact shadows fall (`cast` in the table's `shadows.rs`), so a card at a height
//! casts its shadow at the same place in both tiers. Each frame, the cards are drawn into a
//! depth map seen from the light, through an orthographic projection fitted to the part of the
//! table the camera sees; the table and every sprite cast nothing.
//!
//! The map's size and depth biases are `enhanced.shadow` in `data/beryl3d/space.json`. Its
//! `darkness` is `SHADOW_DARKNESS` in `shadow.wgsl`, because the Enhanced shader is one fixed
//! source; a test holds the two equal, and `taps` to the shader's four.

use std::num::NonZeroU64;

use glam::camera::rh::{proj::directx, view::look_to_mat4};
use glam::{Mat4, Vec3, Vec4, Vec4Swizzles};
use kit_base::data::space::{enhanced::shadow as data, shadow as baseline};
use kit_frame::frame::{Frame, Mesh};

use super::{LIT, LIT_OFFSET, SHADER};
use crate::count::{self, FragmentCounter};
use crate::draw::Draws;
use crate::mesh::Vertex;
use crate::pipeline::{DEPTH, RECORD};
use crate::runs::{Pass, runs};

/// The heights, in millimetres, of the slab of table the map is fitted to: cards lie and fly
/// mostly between them.
const SLAB: [f32; 2] = [0.0, 60.0];
/// How far, in millimetres along the light, the map reaches past the slab's lowest point, so
/// that a table drawn a little below height 0 is inside it.
const BELOW: f32 = 10.0;

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a size in pixels from space.json"
)]
const MAP: u32 = data::MAP as u32;
#[expect(
    clippy::cast_possible_truncation,
    reason = "a small whole number of depth steps from space.json"
)]
const BIAS: i32 = data::BIAS as i32;

/// The mesh attributes `vs_shadow` reads: the position and the uv.
const MESH_ATTRIBUTES: [wgpu::VertexAttribute; 2] = [
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: 0,
        shader_location: 0,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: 8,
        shader_location: 1,
    },
];

/// The record's fields `vs_shadow` reads, at the offsets `Draw` lays them out: the three rows
/// of the transform, `layer` with `material` and `flags`, the tint, whose opacity decides
/// whether the card casts, and the params, whose first is a card's dissolve.
const RECORD_ATTRIBUTES: [wgpu::VertexAttribute; 6] = [
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 0,
        shader_location: 4,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 16,
        shader_location: 5,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 32,
        shader_location: 6,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Uint16x2,
        offset: 56,
        shader_location: 8,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Unorm8x4,
        offset: 60,
        shader_location: 9,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 64,
        shader_location: 10,
    },
];

/// The shadow map and what draws it.
pub(crate) struct Shadow {
    /// The map, bound as bind group 1's binding 1.
    pub(crate) view: wgpu::TextureView,
    /// The depth-only pipeline that draws the cards into the map.
    pipeline: wgpu::RenderPipeline,
    /// The map pass's bind group 1 for each of the renderer's slots, by `Slot::index`, each
    /// naming the `Lit` in that slot's uniform buffer. Made with the map, at startup.
    groups: Vec<wgpu::BindGroup>,
}

impl Shadow {
    /// The shadow map, `enhanced.shadow.map` pixels on a side, its pipeline, and its pass's
    /// bind group for each slot whose uniform buffer is in `uniforms`, in order.
    pub(crate) fn new(device: &wgpu::Device, uniforms: &[&wgpu::Buffer]) -> Self {
        let view = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("shadow map"),
                size: wgpu::Extent3d {
                    width: MAP,
                    height: MAP,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default());
        // The map pass's bind group 1 holds `Lit` alone, since the map cannot be bound while it
        // is drawn into.
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow map lit"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(LIT),
                },
                count: None,
            }],
        });
        let groups = uniforms
            .iter()
            .map(|buffer| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("shadow map lit"),
                    layout: &layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer,
                            offset: LIT_OFFSET,
                            size: NonZeroU64::new(LIT),
                        }),
                    }],
                })
            })
            .collect();
        Self {
            view,
            pipeline: pipeline(device, &layout),
            groups,
        }
    }

    /// Records the shadow map's pass for the frame `draws` describes into `encoder`, and returns
    /// how many draw calls it took: one for each run of card-mesh records before the flat
    /// layer. The vertex stage leaves out the records of any other material, such as the table.
    /// `counter`, when present, counts the pass's fragments, of which a depth-only pass has none.
    pub(crate) fn draw(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        draws: &Draws,
        counter: Option<&FragmentCounter>,
    ) -> u32 {
        let Some(group) = self.groups.get(draws.slot.index) else {
            debug_assert!(
                false,
                "no shadow map bind group for slot {}",
                draws.slot.index
            );
            return 0;
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("shadow map"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(1, group, &[]);
        pass.set_vertex_buffer(0, draws.vertices.slice(..));
        pass.set_index_buffer(draws.indices.slice(..), wgpu::IndexFormat::Uint16);
        let indices = draws.ranges[Mesh::Card as usize].clone();
        let mut draw_calls = 0;
        count::begin(counter, &mut pass);
        for run in runs(draws.frame) {
            if run.pass == Pass::Layer || run.mesh != Mesh::Card as u8 {
                continue;
            }
            let records = run.records.start..run.records.end.min(draws.kept);
            if records.is_empty() {
                continue;
            }
            // As `Draws::record` binds them: from the run's offset, starting at instance 0.
            let bytes = u64::from(records.start) * RECORD..u64::from(records.end) * RECORD;
            pass.set_vertex_buffer(1, draws.slot.instances.slice(bytes));
            pass.draw_indexed(indices.clone(), 0, 0..records.len() as u32);
            draw_calls += 1;
        }
        count::end(counter, &mut pass);
        draw_calls
    }
}

/// The depth-only pipeline that draws cards into the map: `vs_shadow`, no fragment stage, the
/// depth biases of `enhanced.shadow`, and no culling, because a card turned over still casts.
fn pipeline(device: &wgpu::Device, layout: &wgpu::BindGroupLayout) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("shadow map"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    // Bind group 0, which `vs_shadow` does not read, is left out of the layout.
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("shadow map"),
        bind_group_layouts: &[None, Some(layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("shadow map"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_shadow"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[
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
            ],
        },
        fragment: None,
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState {
                constant: BIAS,
                slope_scale: data::SLOPE_BIAS,
                clamp: 0.0,
            },
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

/// The direction toward the key light, in the table's space, where z is height. `cast` moves a
/// point at height h by `light * h` across the table, where `light` is `shadow.light` with its
/// y negated (a shadow falls toward the viewer), so the light lies the opposite way, one
/// millimetre up for each `light` across.
pub(crate) fn key_direction() -> Vec3 {
    let light = baseline::LIGHT;
    Vec3::new(-light.x, light.y, 1.0).normalize()
}

/// From the table's space to the key light's clip space for `frame`: an orthographic projection
/// along the key light, fitted across the light to the slab of table between the heights in
/// `SLAB` that the camera sees, and along it from that slab's lowest point up to the highest
/// point of the camera's frustum, so that every card the camera sees can cast into the map.
pub(crate) fn light_view_projection(frame: &Frame) -> Mat4 {
    let key = key_direction();
    let view = look_to_mat4(Vec3::ZERO, -key, Vec3::Y);
    let (corners, top) = frustum(frame.view_projection);
    let mut low = Vec3::splat(f32::INFINITY);
    let mut high = Vec3::splat(f32::NEG_INFINITY);
    for point in slab(&corners) {
        let seen = view.transform_point3(point);
        // The highest caster that could shade this point lies up the light from it, at `top`.
        let caster = view.transform_point3(point + key * ((top - point.z).max(0.0) / key.z));
        low = low.min(seen).min(caster);
        high = high.max(seen).max(caster);
    }
    if !low.is_finite() || !high.is_finite() {
        // The camera sees none of the slab: nothing it sees lies on the table.
        return Mat4::IDENTITY;
    }
    // The light looks down its negative z, so a point's distance from it is -z.
    directx::orthographic(low.x, high.x, low.y, high.y, -high.z, -low.z + BELOW) * view
}

/// The eight corners of the frustum `view_projection` sees, in the table's space, near
/// corners first, and the height of the highest.
fn frustum(view_projection: Mat4) -> ([Vec3; 8], f32) {
    let inverse = view_projection.inverse();
    let mut corners = [Vec3::ZERO; 8];
    for (index, corner) in corners.iter_mut().enumerate() {
        let x = if index & 1 == 0 { -1.0 } else { 1.0 };
        let y = if index & 2 == 0 { -1.0 } else { 1.0 };
        let z = if index & 4 == 0 { 0.0 } else { 1.0 };
        let point = inverse * Vec4::new(x, y, z, 1.0);
        *corner = point.xyz() / point.w;
    }
    let top = corners
        .iter()
        .map(|corner| corner.z)
        .fold(SLAB[1], f32::max);
    (corners, top)
}

/// The points where the four edges of the frustum with `corners` (near corners first) cross
/// the slab between the heights in `SLAB`: the ends of each edge's part inside the slab.
fn slab(corners: &[Vec3; 8]) -> impl Iterator<Item = Vec3> + '_ {
    (0..4).flat_map(move |edge| {
        let (near, far) = (corners[edge], corners[edge + 4]);
        let rise = far.z - near.z;
        let (enter, leave) = if rise.abs() < f32::EPSILON {
            if (SLAB[0]..=SLAB[1]).contains(&near.z) {
                (0.0, 1.0)
            } else {
                (1.0, 0.0)
            }
        } else {
            let [a, b] = SLAB.map(|height| (height - near.z) / rise);
            (a.min(b).max(0.0), a.max(b).min(1.0))
        };
        let inside = enter <= leave;
        [enter, leave]
            .into_iter()
            .filter(move |_| inside)
            .map(move |t| near.lerp(far, t))
    })
}
