//! The renderer: draws a frame's draw list in the Baseline tier (docs/beryl-3d.md, "The
//! Baseline tier"). The records are uploaded as the instance buffer unchanged, then drawn one
//! instanced call per run of records that share a pass, a mesh, a texture array and a clip:
//! the opaque records with a depth buffer, then the blended ones, tested against it on the
//! table and untested in the flat layer.

use beryl_assets::{Format, Prepared};
use beryl_scene::frame::{Frame, TextureArray, Wake};
use beryl_scene::input::{AssetReady, FaceSize};
use wgpu::util::DeviceExt;

use crate::atlas::Atlas;
use crate::count::FragmentCounter;
use crate::faces::{
    COMPRESSION, FaceArray, Waiting, empty_arrays, face_arrays, face_upload, supported,
};
use crate::mesh::meshes;
use crate::pipeline::{DEPTH, RECORD, bind_group_layout, pipelines};
use crate::runs::runs;
use crate::slots::{Slots, mapped_features};
use crate::target::Target;

/// How many records the instance buffer holds: the most a frame of the engine's largest board
/// holds (`beryl_scene::frame::DRAWS`), so that the buffer is made once, at startup, and never
/// again during play.
const RECORDS: u64 = beryl_scene::frame::DRAWS as u64;
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
    /// The buffers frames write their uniforms and records into, with their bind groups.
    slots: Slots,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    /// The index range of each mesh, by `Mesh` value.
    ranges: [std::ops::Range<u32>; 3],
    /// The board-size and full-size card face arrays, by `FaceSize` value.
    faces: [FaceArray; 2],
    /// The atlas of glyph pages and shapes.
    atlas: Atlas,
    /// Faces that arrived during play, waiting to be uploaded (`hold`).
    waiting: Waiting,
    depth: Option<(wgpu::TextureView, u32, u32)>,
    /// Present when the renderer counts the fragments each frame shades.
    counter: Option<FragmentCounter>,
    /// The GPU adapter's name and backend, for logs.
    pub(crate) adapter: String,
    /// The instance and adapter of a renderer that draws to a surface, for the surfaces a phone
    /// host supplies after losing one.
    pub(crate) gpu: Option<(wgpu::Instance, wgpu::Adapter)>,
}

impl Renderer {
    /// A renderer drawing into targets of `target_format`. The board-size face array starts with
    /// the placeholder back alone, until `set_faces` lists the match's faces.
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
        let faces = empty_arrays(&device, &queue);
        let atlas = Atlas::new(&device, &queue);
        let slots = Slots::new(&device, UNIFORMS, RECORDS * RECORD, |uniforms| {
            bind_groups(&device, &layout, uniforms, &sampler, &faces, &atlas)
        });
        Self {
            device,
            queue,
            format: target_format,
            pipelines,
            layout,
            sampler,
            slots,
            vertices,
            indices,
            ranges: meshes.ranges,
            faces,
            atlas,
            waiting: Waiting::default(),
            depth: None,
            counter: None,
            adapter: "unknown".to_owned(),
            gpu: None,
        }
    }

    /// A renderer on the default GPU adapter with no window or surface, drawing into offscreen
    /// targets of RGBA bytes.
    pub fn headless(limits: Limits) -> Result<Self, String> {
        Self::headless_with(limits, wgpu::Features::empty()).map(|(renderer, _)| renderer)
    }

    /// A headless renderer that counts the fragments each frame shades, read with `fragments`
    /// (docs/beryl-3d.md, "What is counted"). It needs an adapter with pipeline statistics
    /// queries: Vulkan or DirectX 12, not Metal. The gate counts on Mesa's software Vulkan
    /// driver, whose count is exact and repeatable.
    pub fn counting(limits: Limits) -> Result<Self, String> {
        let (mut renderer, adapter) =
            Self::headless_with(limits, wgpu::Features::PIPELINE_STATISTICS_QUERY)?;
        renderer.counter = Some(FragmentCounter::new(&renderer.device, adapter));
        Ok(renderer)
    }

    /// A headless renderer with `features`, and its adapter's name.
    fn headless_with(limits: Limits, features: wgpu::Features) -> Result<(Self, String), String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|error| format!("no GPU adapter: {error}"))?;
        let name = adapter.get_info().name;
        if !adapter.features().contains(features) {
            return Err(format!("the GPU adapter {name:?} lacks {features:?}"));
        }
        // Under the floor's limits the device also leaves out the mapped frame buffers, which
        // OpenGL ES lacks, so that the tests run the path the floor takes.
        let (required_limits, mapped) = match limits {
            Limits::Default => (wgpu::Limits::default(), mapped_features(&adapter)),
            Limits::Downlevel => (
                wgpu::Limits::downlevel_webgl2_defaults(),
                wgpu::Features::empty(),
            ),
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("beryl_render"),
            required_features: features | mapped | (adapter.features() & COMPRESSION),
            required_limits,
            ..Default::default()
        }))
        .map_err(|error| format!("no GPU device: {error}"))?;
        // Stored as sRGB values and blended as they are, as a browser blends colors.
        let mut renderer = Self::new(device, queue, wgpu::TextureFormat::Rgba8Unorm);
        renderer.adapter.clone_from(&name);
        Ok((renderer, name))
    }

    /// The name of the GPU adapter it draws with, as the driver gives it.
    #[must_use]
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    /// The name of the GPU adapter that counts fragments, or `None` for a renderer not made by
    /// `counting`.
    #[must_use]
    pub fn counting_adapter(&self) -> Option<&str> {
        self.counter
            .as_ref()
            .map(|counter| counter.adapter.as_str())
    }

    /// Waits for the GPU and returns how many times the fragment shader ran for the last frame
    /// drawn, or `None` for a renderer not made by `counting`.
    #[must_use]
    pub fn fragments(&self) -> Option<u64> {
        self.counter
            .as_ref()
            .map(|counter| counter.read(&self.device))
    }

    /// Blocks until the GPU has finished every frame submitted so far, for timing a frame from
    /// its submission to its end with the wall clock (docs/beryl-3d.md, "Measurements": GPU
    /// timestamps do not work on the Mac). The app never calls it.
    pub fn finish(&self) {
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the GPU finishes the frame");
    }

    /// The compressed formats the device samples, in the order they are preferred. The face
    /// arrays are made in the first of them (`Format::choose`), and `beryl_assets`' workers are
    /// asked for the same one.
    #[must_use]
    pub fn formats(&self) -> Vec<Format> {
        supported(self.device.features())
    }

    /// Makes both card face arrays for a match, in `format`, which must be one of `formats` or
    /// `Format::Rgba8`. `names` holds one name per face in `FaceId` order, the name drawn on that
    /// face's placeholder; layer 0, the card back's, shows the placeholder back. A face keeps its
    /// placeholder until its layer is uploaded. The full-size array has `FULL_LAYERS` empty
    /// layers. The floor's limits allow 256 layers, so at most 256 faces.
    pub fn set_faces(&mut self, names: &[&str], format: Format) {
        self.faces = face_arrays(&self.device, &self.queue, names, format);
        self.waiting = Waiting::default();
        let (device, layout, sampler) = (&self.device, &self.layout, &self.sampler);
        let (faces, atlas) = (&self.faces, &self.atlas);
        self.slots
            .rebind(|uniforms| bind_groups(device, layout, uniforms, sampler, faces, atlas));
    }

    /// Uploads a face's layer now, into the layer of the array its `AssetReady` names: for the
    /// faces that arrive before the match is shown. Returns whether it was uploaded; anything
    /// but a face, or a face of another format or size than the arrays, is not.
    pub fn upload_layer(&mut self, prepared: &Prepared) -> bool {
        let Some((size, index, layer)) = face_upload(prepared) else {
            return false;
        };
        self.faces[face_array(size)].write(&self.queue, index, layer)
    }

    /// Uploads a glyph page from the glyph worker into its page of the atlas. Returns whether
    /// it was uploaded: anything but a glyph page, or a page past `GLYPH_PAGES`, is not. The
    /// host then gives the client the page's `AssetReady`, so that its text draws.
    pub fn upload_page(&mut self, prepared: &Prepared) -> bool {
        self.atlas.upload(&self.queue, prepared)
    }

    /// Keeps a face that arrived during play until `upload_waiting` lets it through.
    pub fn hold(&mut self, prepared: Prepared) {
        self.waiting.push(prepared);
    }

    /// How many faces `hold` keeps that have not been uploaded yet.
    #[must_use]
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    /// Uploads what this frame may of the faces `hold` keeps, given the client's `wake` as of
    /// its last frame: at most one full-size face, and one board-size face only when the client
    /// is not animating, so that no card changes its picture in the middle of a flight. Returns
    /// what the client must be told of each face uploaded, to give it as `Input::Assets`.
    pub fn upload_waiting(&mut self, wake: Wake) -> impl Iterator<Item = AssetReady> + use<> {
        let mut uploaded = [None, None];
        for (slot, prepared) in uploaded.iter_mut().zip(self.waiting.next(wake)) {
            if let Some(prepared) = prepared
                && self.upload_layer(&prepared)
            {
                *slot = Some(prepared.ready);
            }
        }
        uploaded.into_iter().flatten()
    }

    /// Draws `frame` into `target`, cleared to black first.
    pub fn render(&mut self, frame: &Frame, target: &mut Target) -> Drawn {
        self.render_view(frame, &target.view, target.width, target.height)
    }

    /// Draws `frame` into `view`, `width` by `height` physical pixels, cleared to black first.
    pub(crate) fn render_view(
        &mut self,
        frame: &Frame,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) -> Drawn {
        let mut uniforms = [0.0_f32; (UNIFORMS / 4) as usize];
        uniforms[..16].copy_from_slice(&frame.view_projection.to_cols_array());
        uniforms[16..].copy_from_slice(&[
            width as f32,
            height as f32,
            frame.shake.x,
            frame.shake.y,
        ]);
        // A client made with `Config::LARGEST` never writes more records than the buffer holds.
        // Records past it are not drawn, rather than making a buffer during play.
        let kept = frame.draws.len().min(RECORDS as usize);
        debug_assert_eq!(
            kept,
            frame.draws.len(),
            "a frame holds more than {RECORDS} records"
        );
        let records: &[u8] = bytemuck::cast_slice(&frame.draws[..kept]);
        let depth = self.depth_view(width, height);
        let Some(slot) = self.slots.write(
            &self.device,
            &self.queue,
            bytemuck::cast_slice(&uniforms),
            records,
        ) else {
            // The device was lost; the host makes a new renderer when it notices.
            return Drawn::default();
        };
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let mut drawn = Drawn::default();
        {
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
            if let Some(counter) = &self.counter {
                pass.begin_pipeline_statistics_query(&counter.set, 0);
            }
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);
            let mut state = None;
            for run in runs(frame) {
                let records = run.records.start..run.records.end.min(kept as u32);
                if records.is_empty() {
                    continue;
                }
                let Some(indices) = self.ranges.get(usize::from(run.mesh)) else {
                    debug_assert!(false, "a record names mesh {}", run.mesh);
                    continue;
                };
                let Some(bind_group) = slot.bind_groups.get(usize::from(run.array)) else {
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
                let count = records.len() as u32;
                let bytes = u64::from(records.start) * RECORD..u64::from(records.end) * RECORD;
                pass.set_vertex_buffer(1, slot.instances.slice(bytes));
                pass.draw_indexed(indices.clone(), 0, 0..count);
                drawn.draw_calls += 1;
            }
            if self.counter.is_some() {
                pass.end_pipeline_statistics_query();
            }
        }
        if let Some(counter) = &self.counter {
            counter.resolve(&mut encoder);
        }
        let submission = self.queue.submit([encoder.finish()]);
        self.slots.submitted(submission);
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

/// The index in `Renderer::faces` of the array for `size`.
fn face_array(size: FaceSize) -> usize {
    match size {
        FaceSize::Board => 0,
        FaceSize::Full => 1,
    }
}

fn bind_groups(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    sampler: &wgpu::Sampler,
    faces: &[FaceArray; 2],
    atlas: &Atlas,
) -> [wgpu::BindGroup; 3] {
    let arrays = [&faces[0].view, &faces[1].view, &atlas.view];
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
                    resource: wgpu::BindingResource::TextureView(arrays[array as usize]),
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
