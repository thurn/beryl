//! The renderer: draws a draw list with wgpu, as the doc's Baseline tier says. Two pipelines
//! built from one shader: opaque with a depth buffer, then blended (premultiplied) testing the
//! depth buffer without writing it. One mesh, the rounded card, and one instance buffer holding
//! the 64-byte records as the core wrote them. A third pipeline draws every record blended with
//! no depth buffer, only to compare against.

use glam::Mat4;
use wgpu::util::DeviceExt;

use crate::core::Draw;
use crate::core::draw::{CARD_H, CARD_RADIUS, CARD_W};
use crate::faces::{FACE_H, FACE_W, FaceImages};

pub const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const MAX_RECORDS: u64 = 4096;
/// Points along each rounded corner of the card mesh.
const CORNER_POINTS: usize = 9;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    uv: [f32; 2],
}

/// The card: a rounded rectangle as a triangle fan around its centre, so its corners are
/// geometry and every pixel it covers is opaque.
fn card_mesh() -> (Vec<Vertex>, Vec<u16>) {
    let half = [CARD_W / 2.0 - CARD_RADIUS, CARD_H / 2.0 - CARD_RADIUS];
    let mut vertices = vec![Vertex {
        position: [0.0, 0.0],
        uv: [0.5, 0.5],
    }];
    let corners = [
        (1.0, 1.0, 0.0),
        (-1.0, 1.0, 90.0),
        (-1.0, -1.0, 180.0),
        (1.0, -1.0, 270.0),
    ];
    for (sx, sy, start) in corners {
        for i in 0..CORNER_POINTS {
            let angle = (start + 90.0 * i as f32 / (CORNER_POINTS - 1) as f32).to_radians();
            let x = sx * half[0] + CARD_RADIUS * angle.cos();
            let y = sy * half[1] + CARD_RADIUS * angle.sin();
            vertices.push(Vertex {
                position: [x, y],
                uv: [x / CARD_W + 0.5, 0.5 - y / CARD_H],
            });
        }
    }
    let ring = vertices.len() as u16 - 1;
    let mut indices = Vec::new();
    for i in 0..ring {
        indices.extend_from_slice(&[0, 1 + i, 1 + (i + 1) % ring]);
    }
    (vertices, indices)
}

pub struct Renderer {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    opaque: wgpu::RenderPipeline,
    blended: wgpu::RenderPipeline,
    naive: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniforms: wgpu::Buffer,
    instances: wgpu::Buffer,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    depth: Option<(wgpu::TextureView, u32, u32)>,
    timestamps: Option<Timestamps>,
}

struct Timestamps {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    period: f32,
}

/// What one call to [`Renderer::draw`] did.
pub struct Drawn {
    pub draw_calls: u32,
}

impl Renderer {
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        faces: &FaceImages,
        timestamps: bool,
    ) -> Renderer {
        let layers = faces.layers.len() as u32;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("faces"),
            size: wgpu::Extent3d {
                width: FACE_W,
                height: FACE_H,
                depth_or_array_layers: layers,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (layer, pixels) in faces.layers.iter().enumerate() {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: layer as u32,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(FACE_W * 4),
                    rows_per_image: Some(FACE_H),
                },
                wgpu::Extent3d {
                    width: FACE_W,
                    height: FACE_H,
                    depth_or_array_layers: 1,
                },
            );
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("records"),
            size: MAX_RECORDS * 64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let (mesh, mesh_indices) = card_mesh();
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("card mesh"),
            contents: bytemuck::cast_slice(&mesh),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("card mesh indices"),
            contents: bytemuck::cast_slice(&mesh_indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
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
                    visibility: wgpu::ShaderStages::FRAGMENT,
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
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |blend: Option<wgpu::BlendState>,
                        depth: Option<(bool, wgpu::CompareFunction)>| {
            pipeline(&device, &pipeline_layout, &shader, format, blend, depth)
        };
        let premultiplied = Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING);
        let opaque = pipeline(None, Some((true, wgpu::CompareFunction::Less)));
        let blended = pipeline(
            premultiplied,
            Some((false, wgpu::CompareFunction::LessEqual)),
        );
        let naive = pipeline(premultiplied, None);
        let timestamps = timestamps.then(|| Timestamps {
            set: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: None,
                ty: wgpu::QueryType::Timestamp,
                count: 2,
            }),
            resolve: device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 16,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            read: device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 16,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            period: queue.get_timestamp_period(),
        });
        Renderer {
            device,
            queue,
            opaque,
            blended,
            naive,
            bind_group,
            uniforms,
            instances,
            vertices,
            indices,
            index_count: mesh_indices.len() as u32,
            depth: None,
            timestamps,
        }
    }

    pub fn has_timestamps(&self) -> bool {
        self.timestamps.is_some()
    }

    fn depth_view(&mut self, width: u32, height: u32) -> wgpu::TextureView {
        if let Some((view, w, h)) = &self.depth
            && *w == width
            && *h == height
        {
            return view.clone();
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.depth = Some((view.clone(), width, height));
        view
    }

    /// Records the frame and submits it. `opaque` is how many records at the front of `records`
    /// are opaque. With `naive`, every record is drawn blended with no depth buffer.
    #[expect(clippy::too_many_arguments, reason = "a prototype's one call site")]
    pub fn draw(
        &mut self,
        target: &wgpu::TextureView,
        width: u32,
        height: u32,
        view_proj: Mat4,
        records: &[Draw],
        opaque: usize,
        naive: bool,
    ) -> Drawn {
        let records = &records[..records.len().min(MAX_RECORDS as usize)];
        self.queue.write_buffer(
            &self.uniforms,
            0,
            bytemuck::cast_slice(&view_proj.to_cols_array()),
        );
        self.queue
            .write_buffer(&self.instances, 0, bytemuck::cast_slice(records));
        let depth = (!naive).then(|| self.depth_view(width, height));
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let mut draw_calls = 0;
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: depth.as_ref().map(|view| {
                    wgpu::RenderPassDepthStencilAttachment {
                        view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Discard,
                        }),
                        stencil_ops: None,
                    }
                }),
                timestamp_writes: self.timestamps.as_ref().map(|t| {
                    wgpu::RenderPassTimestampWrites {
                        query_set: &t.set,
                        beginning_of_pass_write_index: Some(0),
                        end_of_pass_write_index: Some(1),
                    }
                }),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            pass.set_vertex_buffer(1, self.instances.slice(..));
            pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);
            let total = records.len() as u32;
            if naive {
                pass.set_pipeline(&self.naive);
                pass.draw_indexed(0..self.index_count, 0, 0..total);
                draw_calls += 1;
            } else {
                let opaque = opaque.min(records.len()) as u32;
                pass.set_pipeline(&self.opaque);
                pass.draw_indexed(0..self.index_count, 0, 0..opaque);
                draw_calls += 1;
                if total > opaque {
                    pass.set_pipeline(&self.blended);
                    pass.draw_indexed(0..self.index_count, 0, opaque..total);
                    draw_calls += 1;
                }
            }
        }
        if let Some(t) = &self.timestamps {
            encoder.resolve_query_set(&t.set, 0..2, &t.resolve, 0);
            encoder.copy_buffer_to_buffer(&t.resolve, 0, &t.read, 0, 16);
        }
        self.queue.submit([encoder.finish()]);
        Drawn { draw_calls }
    }

    /// Waits for the GPU and returns the last frame's pass end timestamp minus its start, in
    /// nanoseconds, if timestamp queries are available. On this Mac's Metal backend the end
    /// comes out earlier than the start, so the sign is kept for the statistics to show.
    pub fn wait(&self) -> Option<i64> {
        let t = self.timestamps.as_ref()?;
        let slice = t.read.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        let ticks = {
            let view = slice.get_mapped_range().ok()?;
            let values: &[u64] = bytemuck::cast_slice(&view);
            if std::env::var_os("BERYL3D_DEBUG_TIMESTAMPS").is_some() {
                eprintln!("timestamps {values:?} period {}", t.period);
            }
            values[1] as i64 - values[0] as i64
        };
        t.read.unmap();
        Some((ticks as f64 * f64::from(t.period)) as i64)
    }

    pub fn wait_idle(&self) {
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }
}

fn pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
    depth: Option<(bool, wgpu::CompareFunction)>,
) -> wgpu::RenderPipeline {
    let mesh = [
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
    let record = [
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: 0,
            shader_location: 2,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: 16,
            shader_location: 3,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: 32,
            shader_location: 4,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Uint16x4,
            offset: 48,
            shader_location: 5,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Uint16x2,
            offset: 56,
            shader_location: 6,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Unorm8x4,
            offset: 60,
            shader_location: 7,
        },
    ];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[
                Some(wgpu::VertexBufferLayout {
                    array_stride: 16,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &mesh,
                }),
                Some(wgpu::VertexBufferLayout {
                    array_stride: 64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &record,
                }),
            ],
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: depth.map(|(write, compare)| wgpu::DepthStencilState {
            format: DEPTH,
            depth_write_enabled: Some(write),
            depth_compare: Some(compare),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}
