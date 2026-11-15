//! The renderer: draws a frame's draw list in the Baseline tier (docs/beryl-3d.md, "The
//! Baseline tier"). The records are uploaded as the instance buffer unchanged, then drawn one
//! instanced call per run of records that share a pass, a mesh, a texture array and a clip:
//! the opaque records with a depth buffer, then the blended ones, tested against it on the
//! table and untested in the flat layer.

use beryl_scene::frame::{Frame, TextureArray};
use wgpu::util::DeviceExt;

use crate::mesh::meshes;
use crate::pipeline::{DEPTH, RECORD, bind_group_layout, pipelines};
use crate::runs::runs;
use crate::target::Target;
use crate::textures::{Layers, placeholder_atlas, placeholder_faces};

/// How many records the instance buffer holds at startup: the tier's upload limit.
const RECORDS: u64 = 4096;
/// The uniforms' size: the view and projection, the target's size and the shake.
const UNIFORMS: u64 = 80;

/// The limits a headless device is created with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limits {
    /// wgpu's default limits.
    Default,
    /// wgpu's downlevel limits for OpenGL ES 3.0 and WebGL 2: the floor's feature set, so that
    /// a shader or buffer the floor could not run fails with a validation error.
    Downlevel,
}

/// What one call to `Renderer::render` did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Drawn {
    pub draw_calls: u32,
}

/// Draws frames. Every pipeline is created by `new`, and none afterwards.
pub struct Renderer {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    pub(crate) format: wgpu::TextureFormat,
    pipelines: [wgpu::RenderPipeline; 3],
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    instances: wgpu::Buffer,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    /// The index range of each mesh, by `Mesh` value.
    ranges: [std::ops::Range<u32>; 3],
    /// The texture arrays, by `TextureArray` value.
    arrays: [wgpu::TextureView; 3],
    /// One bind group per texture array, by `TextureArray` value.
    bind_groups: [wgpu::BindGroup; 3],
    depth: Option<(wgpu::TextureView, u32, u32)>,
}

impl Renderer {
    /// A renderer drawing into targets of `target_format`. Both card face arrays start with
    /// the placeholder back alone, until `set_placeholder_faces` names the cards.
    #[must_use]
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        target_format: wgpu::TextureFormat,
    ) -> Self {
        let layout = bind_group_layout(&device);
        let pipelines = pipelines(&device, &layout, target_format);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("pictures"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: UNIFORMS,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let instances = instance_buffer(&device, RECORDS);
        let meshes = meshes();
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("meshes"),
            contents: bytemuck::cast_slice(&meshes.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh indices"),
            contents: bytemuck::cast_slice(&meshes.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let faces = array(&device, &queue, "faces", &placeholder_faces(&[]));
        let atlas = array(&device, &queue, "atlas", &placeholder_atlas());
        let arrays = [faces.clone(), faces, atlas];
        let bind_groups = bind_groups(&device, &layout, &uniforms, &sampler, &arrays);
        Self {
            device,
            queue,
            format: target_format,
            pipelines,
            layout,
            sampler,
            uniforms,
            instances,
            vertices,
            indices,
            ranges: meshes.ranges,
            arrays,
            bind_groups,
            depth: None,
        }
    }

    /// A renderer on the default GPU adapter with no window or surface, drawing into offscreen
    /// targets of RGBA bytes.
    pub fn headless(limits: Limits) -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|error| format!("no GPU adapter: {error}"))?;
        let required_limits = match limits {
            Limits::Default => wgpu::Limits::default(),
            Limits::Downlevel => wgpu::Limits::downlevel_webgl2_defaults(),
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("beryl_render"),
            required_features: wgpu::Features::empty(),
            required_limits,
            ..Default::default()
        }))
        .map_err(|error| format!("no GPU device: {error}"))?;
        // Stored as sRGB values and blended as they are, as a browser blends colors.
        Ok(Self::new(device, queue, wgpu::TextureFormat::Rgba8Unorm))
    }

    /// Fills both card face arrays with placeholder faces: the back in layer 0 and one face per
    /// name after it, so the card named `names[i]` shows layer `i + 1`.
    /// The floor's limits allow 256 layers, so at most 255 names.
    pub fn set_placeholder_faces(&mut self, names: &[&str]) {
        let faces = array(
            &self.device,
            &self.queue,
            "faces",
            &placeholder_faces(names),
        );
        self.arrays[TextureArray::SmallFaces as usize] = faces.clone();
        self.arrays[TextureArray::FullFaces as usize] = faces;
        self.bind_groups = bind_groups(
            &self.device,
            &self.layout,
            &self.uniforms,
            &self.sampler,
            &self.arrays,
        );
    }

    /// Draws `frame` into `target`, cleared to black first.
    pub fn render(&mut self, frame: &Frame, target: &mut Target) -> Drawn {
        let (width, height) = (target.width, target.height);
        let mut uniforms = [0.0_f32; (UNIFORMS / 4) as usize];
        uniforms[..16].copy_from_slice(&frame.view_projection.to_cols_array());
        uniforms[16..].copy_from_slice(&[
            width as f32,
            height as f32,
            frame.shake.x,
            frame.shake.y,
        ]);
        self.queue
            .write_buffer(&self.uniforms, 0, bytemuck::cast_slice(&uniforms));
        let bytes: &[u8] = bytemuck::cast_slice(&frame.draws);
        if bytes.len() as u64 > self.instances.size() {
            let records = (frame.draws.len() as u64).next_power_of_two();
            self.instances = instance_buffer(&self.device, records);
        }
        if !bytes.is_empty() {
            self.queue.write_buffer(&self.instances, 0, bytes);
        }
        let depth = self.depth_view(width, height);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let mut drawn = Drawn::default();
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth,
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
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);
            let mut state = None;
            for run in runs(frame) {
                let Some(indices) = self.ranges.get(usize::from(run.mesh)) else {
                    debug_assert!(false, "a record names mesh {}", run.mesh);
                    continue;
                };
                let Some(bind_group) = self.bind_groups.get(usize::from(run.array)) else {
                    debug_assert!(false, "a record names texture array {}", run.array);
                    continue;
                };
                if state != Some((run.pass, run.array, run.clip)) {
                    pass.set_pipeline(&self.pipelines[run.pass as usize]);
                    pass.set_bind_group(0, bind_group, &[]);
                    let [x, y, w, h] = scissor(frame, run.clip, width, height);
                    pass.set_scissor_rect(x, y, w, h);
                    state = Some((run.pass, run.array, run.clip));
                }
                // The run's records are bound from their offset, so every draw starts at
                // instance 0: OpenGL ES 3.0 has no first instance.
                let records =
                    u64::from(run.records.start) * RECORD..u64::from(run.records.end) * RECORD;
                pass.set_vertex_buffer(1, self.instances.slice(records));
                pass.draw_indexed(indices.clone(), 0, 0..run.records.len() as u32);
                drawn.draw_calls += 1;
            }
        }
        self.queue.submit([encoder.finish()]);
        drawn
    }

    fn depth_view(&mut self, width: u32, height: u32) -> wgpu::TextureView {
        if let Some((view, w, h)) = &self.depth
            && (*w, *h) == (width, height)
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

fn instance_buffer(device: &wgpu::Device, records: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("records"),
        size: records * RECORD,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// A texture array holding `layers`, as a view of every layer. An array of one layer gets a
/// copy of it as a second, because OpenGL cannot tell a one-layer array from a plain texture.
fn array(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    layers: &Layers,
) -> wgpu::TextureView {
    let mut data = layers.pictures.concat();
    let mut count = layers.pictures.len() as u32;
    if count == 1 {
        data.extend_from_slice(&layers.pictures[0]);
        count = 2;
    }
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: layers.width,
                height: layers.height,
                depth_or_array_layers: count,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &data,
    );
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    })
}

fn bind_groups(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    sampler: &wgpu::Sampler,
    arrays: &[wgpu::TextureView; 3],
) -> [wgpu::BindGroup; 3] {
    [
        TextureArray::SmallFaces,
        TextureArray::FullFaces,
        TextureArray::Atlas,
    ]
    .map(|array| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pictures"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&arrays[array as usize]),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    })
}

const _: () = assert!(size_of::<beryl_scene::frame::Draw>() as u64 == RECORD);
