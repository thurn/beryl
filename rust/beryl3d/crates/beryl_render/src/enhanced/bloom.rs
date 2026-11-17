//! The Enhanced tier's bloom on glows and streaks (docs/beryl-3d.md, "The Enhanced tier":
//! "Bloom"), applied to the resolved picture before the flat layer is drawn over it, so that
//! text and panels never bloom.
//!
//! The records of the Glows band (glow, impact, flash, beam, pulse, trail, the burst of
//! settle and the streaks of paying) are drawn again, with the Baseline shader and additive
//! blending, into a target `enhanced.bloom.scale` of the frame's size. That target is blurred
//! across into a second one and down back into the first, with a Gaussian of
//! `enhanced.bloom.taps` taps and `enhanced.bloom.sigma` texels, and the blurred light is added
//! onto the resolved picture times `enhanced.bloom.strength` (`data/beryl3d/space.json`). The
//! glows are drawn with no depth buffer, so a glow behind a card still blooms around it.

use beryl_scene::data::space::enhanced::bloom::{SCALE, SIGMA, STRENGTH, TAPS};
use beryl_scene::frame::Band;
use wgpu::util::DeviceExt;

use crate::draw::Draws;
use crate::pipeline::VERTEX_BUFFERS;
use crate::runs::Pass;

/// How many taps lie on each side of the blur's centre.
const RADIUS: usize = TAPS / 2;
/// How many weights `Uniforms::weights` holds.
const WEIGHTS: usize = 16;
const _: () = assert!(TAPS % 2 == 1 && RADIUS < WEIGHTS);

/// `Bloom` in bloom.wgsl: one pass's uniforms.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    direction: [f32; 2],
    radius: u32,
    strength: f32,
    weights: [f32; WEIGHTS],
}

impl Uniforms {
    /// The blur along `direction`, a step of one texel.
    fn blur(direction: [f32; 2]) -> Self {
        let mut weights = [0.0; WEIGHTS];
        for (i, weight) in weights.iter_mut().enumerate().take(RADIUS + 1) {
            let i = i as f32;
            *weight = (-i * i / (2.0 * SIGMA * SIGMA)).exp();
        }
        // The centre counts once and every other tap twice, once on each side.
        let total = weights[0] + 2.0 * weights[1..].iter().sum::<f32>();
        for weight in &mut weights {
            *weight /= total;
        }
        Self {
            direction,
            radius: RADIUS as u32,
            strength: 0.0,
            weights,
        }
    }

    fn composite() -> Self {
        Self {
            direction: [0.0; 2],
            radius: 0,
            strength: STRENGTH,
            weights: [0.0; WEIGHTS],
        }
    }
}

/// The two small targets, and the bind groups that read them, for a frame of one size.
struct Targets {
    /// The glows are drawn here, and the blur down writes the blurred light back here.
    first: wgpu::TextureView,
    /// The blur across writes here.
    second: wgpu::TextureView,
    /// Reads `first`, for the blur across.
    across: wgpu::BindGroup,
    /// Reads `second`, for the blur down.
    down: wgpu::BindGroup,
    /// Reads `first`, for the composite.
    composite: wgpu::BindGroup,
    /// The frame's size these targets were made for.
    frame: (u32, u32),
    /// The targets' own size.
    size: (u32, u32),
}

/// Bloom's textures and pipelines.
pub(crate) struct Bloom {
    /// Draws the Glows band's records into `Targets::first`.
    glows: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The uniforms of the blur across, the blur down and the composite.
    uniforms: [wgpu::Buffer; 3],
    targets: Option<Targets>,
    format: wgpu::TextureFormat,
}

impl Bloom {
    /// Bloom for targets of `format`, drawing records with bind group 0 laid out as `records`.
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        records: &wgpu::BindGroupLayout,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bloom"),
            source: wgpu::ShaderSource::Wgsl(include_str!("bloom.wgsl").into()),
        });
        let layout = layout(device);
        let additive = wgpu::BlendState {
            color: ADD,
            alpha: ADD,
        };
        let glows = glows_pipeline(device, format, records, additive);
        let screen_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bloom"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let screen = |label, entry_point, blend, write_mask| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&screen_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_screen"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry_point),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let blur = screen("bloom blur", "blur", None, wgpu::ColorWrites::ALL);
        // The light adds to the picture's color and leaves its alpha alone.
        let composite = screen(
            "bloom composite",
            "composite",
            Some(additive),
            wgpu::ColorWrites::COLOR,
        );
        let uniforms = [
            Uniforms::blur([1.0, 0.0]),
            Uniforms::blur([0.0, 1.0]),
            Uniforms::composite(),
        ]
        .map(|uniforms| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("bloom"),
                contents: bytemuck::bytes_of(&uniforms),
                usage: wgpu::BufferUsages::UNIFORM,
            })
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("bloom"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            glows,
            blur,
            composite,
            layout,
            sampler,
            uniforms,
            targets: None,
            format,
        }
    }

    /// Records bloom for the frame `draws` describes into `encoder`, over the resolved picture
    /// in `view`, and returns how many draw calls it took: none, recording nothing, when the
    /// frame has no glows; otherwise the glows' runs and one call for each of the blur across,
    /// the blur down and the composite.
    pub(crate) fn apply(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        draws: &Draws,
        view: &wgpu::TextureView,
    ) -> u32 {
        let bands = &draws.frame.bands;
        let glows = bands[Band::Glows as usize]..bands[Band::Glows as usize + 1].min(draws.kept);
        if glows.is_empty() {
            return 0;
        }
        self.size_targets(device, draws.width, draws.height);
        let Some(targets) = &self.targets else {
            return 0;
        };
        let mut draw_calls = {
            let mut pass = begin(encoder, "bloom glows", &targets.first, true);
            draws.record_part(
                &mut pass,
                glows,
                |pass| (pass == Pass::Table).then_some(&self.glows),
                targets.size,
            )
        };
        let steps = [
            ("bloom across", &targets.second, &targets.across, &self.blur),
            ("bloom down", &targets.first, &targets.down, &self.blur),
            ("bloom composite", view, &targets.composite, &self.composite),
        ];
        for (label, into, reads, pipeline) in steps {
            // The composite loads the picture it adds to; each blur writes every texel.
            let clear = !std::ptr::eq(into, view);
            let mut pass = begin(encoder, label, into, clear);
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, reads, &[]);
            pass.draw(0..3, 0..1);
            draw_calls += 1;
        }
        draw_calls
    }

    /// Makes the small targets for a frame `width` by `height`, again only when the size
    /// changes.
    fn size_targets(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if self
            .targets
            .as_ref()
            .is_none_or(|targets| targets.frame != (width, height))
        {
            let size = (
                ((width as f32 * SCALE).ceil() as u32).max(1),
                ((height as f32 * SCALE).ceil() as u32).max(1),
            );
            let texture = |label| {
                device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some(label),
                        size: wgpu::Extent3d {
                            width: size.0,
                            height: size.1,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: self.format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    })
                    .create_view(&wgpu::TextureViewDescriptor::default())
            };
            let first = texture("bloom first");
            let second = texture("bloom second");
            let [across, down, composite] = &self.uniforms;
            self.targets = Some(Targets {
                across: self.group(device, across, &first),
                down: self.group(device, down, &second),
                composite: self.group(device, composite, &first),
                first,
                second,
                frame: (width, height),
                size,
            });
        }
    }

    /// The bind group of a pass with `uniforms` that reads `source`.
    fn group(
        &self,
        device: &wgpu::Device,
        uniforms: &wgpu::Buffer,
        source: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bloom"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }

    /// Frees the small targets, which the next frame with glows makes again.
    pub(crate) fn low_memory(&mut self) {
        self.targets = None;
    }
}

/// The pipeline that draws the Glows band's records into the first small target: the Baseline
/// shader, one sample, `additive` blending and no depth buffer.
fn glows_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    records: &wgpu::BindGroupLayout,
    additive: wgpu::BlendState,
) -> wgpu::RenderPipeline {
    let baseline = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("bloom glows"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shader.wgsl").into()),
    });
    let glows_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("bloom glows"),
        bind_group_layouts: &[Some(records)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("bloom glows"),
        layout: Some(&glows_layout),
        vertex: wgpu::VertexState {
            module: &baseline,
            entry_point: Some("vs"),
            compilation_options: wgpu::PipelineCompilationOptions {
                // The glows are on the table, which is shaken.
                constants: &[("SHAKEN", 1.0)],
                ..Default::default()
            },
            buffers: &VERTEX_BUFFERS,
        },
        fragment: Some(wgpu::FragmentState {
            module: &baseline,
            entry_point: Some("fs"),
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

/// Adds the source to the destination.
const ADD: wgpu::BlendComponent = wgpu::BlendComponent {
    src_factor: wgpu::BlendFactor::One,
    dst_factor: wgpu::BlendFactor::One,
    operation: wgpu::BlendOperation::Add,
};

/// A render pass into `view` with no depth buffer, cleared to transparent black first when
/// `clear` is set and loading what is there otherwise.
fn begin<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    label: &str,
    view: &wgpu::TextureView,
    clear: bool,
) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: if clear {
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                } else {
                    wgpu::LoadOp::Load
                },
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

/// The layout of a blur or composite pass's bind group: its uniforms, the texture it reads
/// and a filtering sampler.
fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("bloom"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
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
        ],
    })
}
