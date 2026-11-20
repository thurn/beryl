//! The backdrop filter (`beryl_scene::frame::Backdrop`): the records before the filter are drawn
//! into an offscreen picture the size of the frame, which is blurred across into a second
//! picture, then blurred down into the frame's target with the color filters, and the rest of
//! the flat layer is drawn over it. Only the web interface's end screen asks for one, so the two
//! pictures are made by the first frame that does, and kept until the size changes or memory is
//! low.
//!
//! The blur is a Gaussian of the frame's deviation, reaching three deviations each side, on sRGB
//! values, as `WebKit` blurs; the target's linear values are encoded as sRGB first and decoded
//! after the filters. The picture blurred across holds sRGB values in 8 bits, which every
//! device can draw into.

use beryl_scene::frame::Backdrop;
use wgpu::util::DeviceExt;

/// The format of the picture blurred across: sRGB values, stored as they are.
const ACROSS: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// The uniforms of one pass, `Blur` in `backdrop.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    step: [i32; 2],
    radius: i32,
    decode: u32,
    sigma: f32,
    encode: u32,
    pad: [f32; 2],
    rows: [[f32; 4]; 3],
}

const _: () = assert!(size_of::<Uniforms>() == 80);

/// The two offscreen pictures for frames of one size, with the bind groups that read them.
struct Pictures {
    /// What the records before the filter draw into.
    drawn: wgpu::TextureView,
    across: wgpu::TextureView,
    /// The pass across reads `drawn`, and the pass down reads `across`.
    groups: [wgpu::BindGroup; 2],
    size: (u32, u32),
}

/// The backdrop filter's pipelines, uniforms and pictures.
pub(crate) struct Filter {
    /// The pass across, into `ACROSS`, and the pass down, into the frame's format.
    pipelines: [wgpu::RenderPipeline; 2],
    layout: wgpu::BindGroupLayout,
    uniforms: [wgpu::Buffer; 2],
    /// The frame's target format, which `drawn` takes too.
    format: wgpu::TextureFormat,
    pictures: Option<Pictures>,
}

impl Filter {
    /// The backdrop filter for frames drawn into targets of `format`.
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("backdrop"),
            source: wgpu::ShaderSource::Wgsl(include_str!("backdrop.wgsl").into()),
        });
        let layout = layout(device);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("backdrop"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |label, format| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_screen"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_filter"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipelines = [
            pipeline("backdrop across", ACROSS),
            pipeline("backdrop down", format),
        ];
        let uniforms = ["backdrop across", "backdrop down"].map(|label| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::bytes_of(&<Uniforms as bytemuck::Zeroable>::zeroed()),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            })
        });
        Self {
            pipelines,
            layout,
            uniforms,
            format,
            pictures: None,
        }
    }

    /// The picture the records before the filter are drawn into, for a frame `width` by
    /// `height`, made when there is none of that size.
    pub(crate) fn drawn(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> wgpu::TextureView {
        if let Some(pictures) = &self.pictures
            && pictures.size == (width, height)
        {
            return pictures.drawn.clone();
        }
        let picture = |label, format| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let drawn = picture("backdrop drawn", self.format);
        let across = picture("backdrop across", ACROSS);
        let group = |uniforms: &wgpu::Buffer, source| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("backdrop"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(source),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: uniforms.as_entire_binding(),
                    },
                ],
            })
        };
        let groups = [
            group(&self.uniforms[0], &drawn),
            group(&self.uniforms[1], &across),
        ];
        self.pictures = Some(Pictures {
            drawn: drawn.clone(),
            across,
            groups,
            size: (width, height),
        });
        drawn
    }

    /// Records the blur across and the blur down with the color filters of `backdrop` into
    /// `encoder`, from the picture `drawn` returned into `view`, which every texel of is
    /// written. Returns the draw calls it took: one a pass.
    pub(crate) fn apply(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        backdrop: &Backdrop,
        view: &wgpu::TextureView,
    ) -> u32 {
        let Some(pictures) = &self.pictures else {
            debug_assert!(
                false,
                "the backdrop's pictures are made before it is applied"
            );
            return 0;
        };
        let sigma = backdrop.blur.max(0.0);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "three deviations of a blur of a few points"
        )]
        let radius = (3.0 * sigma).ceil() as i32;
        let linear = self.format.is_srgb();
        let identity = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
        let rows = backdrop.color.map(|[r, g, b]| [r, g, b, 0.0]);
        let passes = [
            Uniforms {
                step: [1, 0],
                radius,
                decode: u32::from(linear),
                sigma,
                encode: 0,
                pad: [0.0; 2],
                rows: identity,
            },
            Uniforms {
                step: [0, 1],
                radius,
                decode: 0,
                sigma,
                encode: u32::from(linear),
                pad: [0.0; 2],
                rows,
            },
        ];
        let targets = [&pictures.across, view];
        for (at, uniforms) in passes.iter().enumerate() {
            queue.write_buffer(&self.uniforms[at], 0, bytemuck::bytes_of(uniforms));
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("backdrop"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: targets[at],
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipelines[at]);
            pass.set_bind_group(0, &pictures.groups[at], &[]);
            pass.draw(0..3, 0..1);
        }
        2
    }

    /// Frees the pictures, which the next frame with a backdrop filter makes again.
    pub(crate) fn low_memory(&mut self) {
        self.pictures = None;
    }
}

/// The layout of a pass's bind group: the picture it reads, texel by texel, and its uniforms.
fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("backdrop"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    })
}
