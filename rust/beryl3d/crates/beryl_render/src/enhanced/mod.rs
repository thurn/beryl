//! The Enhanced tier's renderer (docs/beryl-3d.md, "The Enhanced tier"): the same draw list as
//! the Baseline tier, drawn into a four-sample target and resolved, with a shadow map, lighting
//! and bloom. Every pipeline it needs is made with it, at startup.
//!
//! A frame is drawn in four steps: the shadow map (`shadow::Shadow::draw`); the main pass over
//! every record before the flat layer, multisampled and resolved into the frame's target; bloom
//! (`bloom::Bloom::apply`), on the resolved picture; then the flat layer, in a pass of one sample
//! that loads the resolved picture, with the Baseline tier's flat-layer pipeline, so that text
//! and panels are drawn exactly as in Baseline.
//!
//! The shading of each step lives in a file of its own, Rust and WGSL alike: `shadow`,
//! `light` and `bloom`.

mod bloom;
mod light;
mod shadow;

use std::num::NonZeroU64;

use beryl_scene::frame::Frame;

use crate::draw::Draws;
use crate::pipeline::{DEPTH, Shape, pipeline_set};
use crate::runs::Pass;

use self::bloom::Bloom;
use self::shadow::Shadow;

/// How many samples each pixel of the main pass holds.
pub(crate) const SAMPLES: u32 = 4;
/// Where `Lit` starts in a frame's uniform buffer: after the Baseline uniforms, at the largest
/// offset alignment a device may ask of a uniform binding.
pub(crate) const LIT_OFFSET: u64 = 256;
/// The size of `Lit`.
const LIT: u64 = size_of::<Lit>() as u64;

/// The Enhanced shader: the Baseline shader, whose functions it reuses, followed by the Enhanced
/// entry points and shading. Each Enhanced material is shaded by a function named
/// `shade_<material>_lit`, or `shade_particle`, which the shader cost test counts by name.
pub const SHADER: &str = concat!(
    include_str!("../shader.wgsl"),
    include_str!("entry.wgsl"),
    include_str!("light.wgsl"),
    include_str!("shadow.wgsl"),
    include_str!("particle.wgsl"),
);

/// The Enhanced uniforms, bind group 1's binding 0, written each frame after the Baseline ones.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Lit {
    /// From the table's space to the key light's clip space, for the shadow map.
    light_view_projection: [f32; 16],
    /// The direction toward the key light; w is unused.
    key: [f32; 4],
    /// The camera's position in the table's space; w is unused.
    eye: [f32; 4],
    /// The fill light's color and strength.
    fill: [f32; 4],
}

impl Lit {
    /// The lights of `frame`.
    pub(crate) fn of(frame: &Frame) -> Self {
        let lighting = light::lighting(frame);
        Self {
            light_view_projection: shadow::light_view_projection(frame).to_cols_array(),
            key: lighting.key.to_array(),
            eye: lighting.eye.to_array(),
            fill: lighting.fill.to_array(),
        }
    }
}

const _: () = assert!(LIT == 112);

/// The four-sample color and depth targets of the main pass, and their size.
struct Targets {
    color: wgpu::TextureView,
    depth: wgpu::TextureView,
    width: u32,
    height: u32,
}

/// The Enhanced tier's pipelines and the textures they draw with.
pub(crate) struct Enhanced {
    /// The pipelines for the three passes, indexed by `Pass as usize`, with four samples. The
    /// flat layer is drawn with the Baseline tier's one-sample pipeline instead, after the
    /// resolve; its four-sample pipeline is made with the others so that a frame drawn some
    /// other way later needs none made during play.
    pipelines: [wgpu::RenderPipeline; 3],
    /// The layout of bind group 1: `Lit`, the shadow map and its comparison sampler.
    layout: wgpu::BindGroupLayout,
    shadow: Shadow,
    bloom: Bloom,
    comparison: wgpu::Sampler,
    /// Bind group 1 for each of the renderer's slots, by `Slot::index`: each names the `Lit`
    /// in that slot's uniform buffer.
    groups: Vec<wgpu::BindGroup>,
    targets: Option<Targets>,
    format: wgpu::TextureFormat,
}

impl Enhanced {
    /// The Enhanced tier for a device drawing into targets of `format`, or `None` when the
    /// device was made with less than wgpu's default limits (the floor's, `Limits::Downlevel`)
    /// or cannot draw four samples into `format` and `DEPTH`. `layout` is bind group 0's, and
    /// `uniforms` the uniform buffer of each of the renderer's slots, in order.
    pub(crate) fn new<'a>(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        layout: &wgpu::BindGroupLayout,
        uniforms: impl Iterator<Item = &'a wgpu::Buffer>,
    ) -> Option<Self> {
        if !supported(device, format) {
            return None;
        }
        let lit_layout = lit_layout(device);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("enhanced"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let shape = Shape {
            shader: &shader,
            entry_points: ("vs_lit", "fs_lit"),
            format,
            samples: SAMPLES,
        };
        let pipelines = pipeline_set(device, &[layout, &lit_layout], &shape);
        let comparison = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow map"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let mut enhanced = Self {
            pipelines,
            layout: lit_layout,
            shadow: Shadow::new(device, queue),
            bloom: Bloom::new(),
            comparison,
            groups: Vec::new(),
            targets: None,
            format,
        };
        enhanced.groups = uniforms
            .map(|buffer| enhanced.group(device, buffer))
            .collect();
        Some(enhanced)
    }

    /// Bind group 1 for the slot whose uniform buffer is `uniforms`.
    fn group(&self, device: &wgpu::Device, uniforms: &wgpu::Buffer) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("lit"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: uniforms,
                        offset: LIT_OFFSET,
                        size: NonZeroU64::new(LIT),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&self.shadow.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.comparison),
                },
            ],
        })
    }

    /// Records the frame `draws` describes into `encoder`, ending in `view`, and returns how
    /// many draw calls every pass took. `layer` is the Baseline tier's pipelines, of which the
    /// flat layer's is used, and `layer_depth` a one-sample depth buffer the size of `view`.
    pub(crate) fn render(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        draws: &Draws,
        view: &wgpu::TextureView,
        layer: &[wgpu::RenderPipeline; 3],
        layer_depth: &wgpu::TextureView,
    ) -> u32 {
        let mut draw_calls = self.shadow.draw(encoder, draws);
        let targets = self.targets(device, draws.width, draws.height);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("enhanced"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.color,
                    depth_slice: None,
                    resolve_target: Some(view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        // Only the resolved picture is kept.
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(depth_attachment(&targets.depth)),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let Some(group) = self.groups.get(draws.slot.index) else {
                debug_assert!(false, "no bind group for slot {}", draws.slot.index);
                return draw_calls;
            };
            pass.set_bind_group(1, group, &[]);
            draw_calls += draws.record(&mut pass, &self.pipelines, |pass| pass != Pass::Layer);
        }
        draw_calls += self.bloom.apply(encoder, draws, view);
        if draws.frame.layer < draws.kept {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("flat layer"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(depth_attachment(layer_depth)),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            draw_calls += draws.record(&mut pass, layer, |pass| pass == Pass::Layer);
        }
        draw_calls
    }

    /// The main pass's targets for a target `width` by `height`, made again only when the size
    /// changes.
    fn targets(&mut self, device: &wgpu::Device, width: u32, height: u32) -> &Targets {
        if self
            .targets
            .as_ref()
            .is_none_or(|targets| (targets.width, targets.height) != (width, height))
        {
            let texture = |label, format| {
                device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some(label),
                        size: wgpu::Extent3d {
                            width,
                            height,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: SAMPLES,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                        view_formats: &[],
                    })
                    .create_view(&wgpu::TextureViewDescriptor::default())
            };
            self.targets = Some(Targets {
                color: texture("enhanced color", self.format),
                depth: texture("enhanced depth", DEPTH),
                width,
                height,
            });
        }
        self.targets.as_ref().expect("just made")
    }

    /// Frees the main pass's targets, which the next frame makes again (docs/beryl-3d.md,
    /// "Hosts": "Low memory").
    pub(crate) fn low_memory(&mut self) {
        self.targets = None;
    }
}

/// A depth attachment cleared to the far plane and not kept after its pass.
fn depth_attachment(view: &wgpu::TextureView) -> wgpu::RenderPassDepthStencilAttachment<'_> {
    wgpu::RenderPassDepthStencilAttachment {
        view,
        depth_ops: Some(wgpu::Operations {
            load: wgpu::LoadOp::Clear(1.0),
            store: wgpu::StoreOp::Discard,
        }),
        stencil_ops: None,
    }
}

/// Whether `device` can draw the Enhanced tier into targets of `format`: it has at least wgpu's
/// default limits, and both `format` and `DEPTH` take four samples, with `format` resolving
/// them. Without adapter-specific format features, wgpu reports the formats' features as
/// WebGPU guarantees them.
fn supported(device: &wgpu::Device, format: wgpu::TextureFormat) -> bool {
    let features = device.features();
    let color = format.guaranteed_format_features(features).flags;
    let depth = DEPTH.guaranteed_format_features(features).flags;
    wgpu::Limits::default().check_limits(&device.limits())
        && color.sample_count_supported(SAMPLES)
        && color.contains(wgpu::TextureFormatFeatureFlags::MULTISAMPLE_RESOLVE)
        && depth.sample_count_supported(SAMPLES)
}

/// The layout of bind group 1: `Lit`, the shadow map and its comparison sampler.
fn lit_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lit"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(LIT),
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                count: None,
            },
        ],
    })
}
