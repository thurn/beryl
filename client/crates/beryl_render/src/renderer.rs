//! The renderer: draws a frame's draw list in the Baseline tier (docs/beryl-3d.md, "The
//! Baseline tier"), or in the Enhanced tier when the frame asks for it and the device allows it
//! (`enhanced`). The records are uploaded as the instance buffer unchanged, then drawn one
//! instanced call per run of records that share a pass, a mesh, a texture array and a clip:
//! the opaque records with a depth buffer, then the blended ones, tested against it on the
//! table and untested in the flat layer.

use beryl_assets::{Format, Placeholders, Prepared, dimensions};
use kit_base::atlas::{AssetReady, FaceSize, Tier};
use kit_base::data::lighting::Lighting;
use kit_frame::frame::{Frame, Mesh, TextureArray, Wake};
use wgpu::util::DeviceExt;

use crate::atlas::Atlas;
use crate::backdrop::Filter;
use crate::count::{self, FragmentCounter};
use crate::draw::Draws;
use crate::enhanced::{Enhanced, LIT_OFFSET, Lit};
use crate::faces::{
    COMPRESSION, FaceArray, Waiting, empty_arrays, face_arrays, face_upload, full_array, supported,
    token_array,
};
use crate::mesh::meshes;
use crate::panel_blur::{self, PanelBlur};
use crate::pipeline::{DEPTH, Pipelines, RECORD, bind_group_layout, pipelines};
use crate::slots::{Slots, mapped_features};
use crate::target::Target;
use glam::Vec2;
use kit_frame::frame::PaintTable;

/// How many records the instance buffer holds: the most a frame of the engine's largest board
/// holds (`kit_frame::frame::DRAWS`), so that the buffer is made once, at startup, and never
/// again during play.
const RECORDS: u64 = kit_frame::frame::DRAWS as u64;
/// The Baseline uniforms' size: the view and projection, the target's size, the shake, and
/// whether the frame has masks.
const UNIFORMS: u64 = 96;
/// Where the frame's paint table (`kit_frame::frame::PaintTable`) starts in the uniform
/// buffer: after the Enhanced tier's `Lit`, at the next offset a uniform binding may start at.
const PAINT_OFFSET: u64 = 512;
/// The paint table's size.
pub(crate) const PAINT_TABLE: u64 = size_of::<PaintTable>() as u64;
/// The size of a frame's uniform buffer: the Baseline uniforms, the Enhanced tier's `Lit` at
/// `LIT_OFFSET`, then the paint table at `PAINT_OFFSET`.
const UNIFORM_BUFFER: u64 = PAINT_OFFSET + PAINT_TABLE;

const _: () = assert!(LIT_OFFSET + size_of::<Lit>() as u64 <= PAINT_OFFSET);

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
    pipelines: Pipelines,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The buffers frames write their uniforms and records into, with their bind groups.
    pub(crate) slots: Slots,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    /// The index range of each mesh, by `Mesh` value.
    ranges: [std::ops::Range<u32>; Mesh::COUNT],
    /// The card size the card meshes in `vertices` were built at.
    card: Vec2,
    /// The board-size and full-size card face arrays, by `FaceSize` value.
    faces: [FaceArray; 2],
    /// The atlas of glyph pages and shapes.
    atlas: Atlas,
    /// Faces that arrived during play, waiting to be uploaded (`hold`).
    waiting: Waiting,
    /// The format of the full-size array that `low_memory` freed, which the next full-size
    /// upload makes again, empty.
    freed_full: Option<Format>,
    depth: Option<(wgpu::TextureView, u32, u32)>,
    /// The Enhanced tier, on a device that can draw it.
    pub(crate) enhanced: Option<Enhanced>,
    /// The backdrop filter, for frames that ask for one (`Frame::backdrop`).
    backdrop: Filter,
    inset: crate::inset::Compositor,
    panel_blur: Option<PanelBlur>,
    /// Present when the renderer counts the fragments each frame shades.
    pub(crate) counter: Option<FragmentCounter>,
    /// The GPU adapter's name and backend, for logs.
    pub(crate) adapter: String,
    /// The instance and adapter of a renderer that draws to a surface, for the surfaces a phone
    /// host supplies after losing one.
    pub(crate) gpu: Option<(wgpu::Instance, wgpu::Adapter)>,
}

impl Renderer {
    /// A renderer drawing into targets of `target_format`, with the Enhanced tier's lights,
    /// shadow map and bloom from `lighting` (`data/client/lighting.json`). The board-size face
    /// array starts with the placeholder back alone, until `set_faces` lists the match's faces.
    #[must_use]
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        target_format: wgpu::TextureFormat,
        lighting: Lighting,
    ) -> Self {
        let layout = bind_group_layout(&device);
        let pipelines = pipelines(&device, &layout, target_format);
        // Card faces are drawn smaller than their largest mipmap level almost everywhere, so
        // the sampler blends the two nearest levels (trilinear) rather than snapping to one: a
        // card a little under two thirds of its face's size would otherwise be drawn from the
        // half-size level, enlarged. Anisotropic filtering keeps a card that the camera's tilt
        // foreshortens sharp along its other axis. wgpu ignores the clamp on a device without
        // anisotropic filtering. The shader biases the level toward the larger one
        // (`PICTURE_LOD_BIAS` in `shader.wgsl`).
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("pictures"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 4,
            ..Default::default()
        });
        // A frame's card size until the first frame says otherwise (`Frame::card`).
        let card = Vec2::ONE;
        let meshes = meshes(card);
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("meshes"),
            contents: bytemuck::cast_slice(&meshes.vertices),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh indices"),
            contents: bytemuck::cast_slice(&meshes.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let faces = empty_arrays(&device, &queue);
        let atlas = Atlas::new(&device);
        let slots = Slots::new(&device, UNIFORM_BUFFER, RECORDS * RECORD, |uniforms| {
            bind_groups(&device, &layout, uniforms, &sampler, &faces, &atlas)
        });
        let enhanced = Enhanced::new(&device, target_format, &layout, slots.uniforms(), lighting);
        let backdrop = Filter::new(&device, target_format);
        let inset = crate::inset::Compositor::new(&device, target_format);
        let panel_blur = enhanced
            .as_ref()
            .map(|_| PanelBlur::new(&device, target_format));
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
            card,
            faces,
            atlas,
            waiting: Waiting::default(),
            freed_full: None,
            depth: None,
            enhanced,
            backdrop,
            inset,
            panel_blur,
            counter: None,
            adapter: "unknown".to_owned(),
            gpu: None,
        }
    }

    /// A renderer on the default GPU adapter with no window or surface, drawing into offscreen
    /// targets of RGBA bytes, lit by `lighting`.
    pub fn headless(limits: Limits, lighting: Lighting) -> Result<Self, String> {
        Self::headless_with(limits, wgpu::Features::empty(), lighting).map(|(renderer, _)| renderer)
    }

    /// A headless renderer that counts the fragments each frame shades, read with `fragments`
    /// (docs/beryl-3d.md, "What is counted"). It needs an adapter with pipeline statistics
    /// queries: Vulkan or DirectX 12, not Metal. The gate counts on Mesa's software Vulkan
    /// driver, whose count is exact and repeatable.
    pub fn counting(limits: Limits, lighting: Lighting) -> Result<Self, String> {
        let (mut renderer, adapter) =
            Self::headless_with(limits, wgpu::Features::PIPELINE_STATISTICS_QUERY, lighting)?;
        renderer.counter = Some(FragmentCounter::new(&renderer.device, adapter));
        Ok(renderer)
    }

    /// A headless renderer with `features`, lit by `lighting`, and its adapter's name.
    fn headless_with(
        limits: Limits,
        features: wgpu::Features,
        lighting: Lighting,
    ) -> Result<(Self, String), String> {
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
        let mut renderer = Self::new(device, queue, wgpu::TextureFormat::Rgba8Unorm, lighting);
        renderer.adapter.clone_from(&name);
        Ok((renderer, name))
    }

    /// Whether it draws frames in `Tier::Enhanced` in that tier. A renderer that cannot, on a
    /// device made with the floor's limits or without four-sample targets, draws them in
    /// Baseline.
    #[must_use]
    pub fn enhanced_available(&self) -> bool {
        self.enhanced.is_some()
    }

    /// Lights later frames with `lighting`, as when a watched data file changes. Nothing is
    /// made when it equals the tuning in use, so a host may call it before every frame.
    pub fn set_lighting(&mut self, lighting: Lighting) {
        if let Some(enhanced) = &mut self.enhanced
            && enhanced.tuning != lighting
        {
            enhanced.retune(&self.device, &self.layout, self.slots.uniforms(), lighting);
        }
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

    /// Makes both card face arrays for a match, in the placeholders' format, which must be one
    /// of `formats` or `Format::Rgba8`. `placeholders` holds one finished layer per face in
    /// `FaceId` order, which the asset workers drew (`Preparer::placeholders`); layer 0 is the
    /// card back's. They are copied as they are, so the frame thread draws none. A face keeps its
    /// placeholder until its layer is uploaded. The full-size array has `FULL_LAYERS` empty
    /// layers. The floor's limits allow 256 layers, so at most 256 faces.
    pub fn set_faces(&mut self, placeholders: &Placeholders) {
        self.faces = face_arrays(&self.device, &self.queue, placeholders);
        self.waiting = Waiting::default();
        self.freed_full = None;
        self.rebind();
    }

    fn rebind(&mut self) {
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
        if size == FaceSize::Full
            && let Some(format) = self.freed_full.take()
        {
            self.faces[face_array(size)] = full_array(&self.device, format);
            self.rebind();
        }
        self.faces[face_array(size)].write(&self.queue, index, layer)
    }

    /// How many layers of the array for `size` hold a face uploaded since the array was made.
    #[must_use]
    pub fn uploaded(&self, size: FaceSize) -> usize {
        self.faces[face_array(size)].filled()
    }

    /// Whether the array holds an uploaded face in this layer.
    #[must_use]
    pub fn layer_uploaded(&self, size: FaceSize, layer: u16) -> bool {
        self.faces[face_array(size)].layer_uploaded(layer)
    }

    /// Frees what can be made again when the platform says memory is low (docs/beryl-3d.md,
    /// "Hosts": "Low memory"): the full-size face array, which the next full-size upload makes
    /// again, empty, and the full-size faces waiting to be uploaded. The client forgets its
    /// full-size layers on the same `Lifecycle::LowMemory`, so those cards draw from their
    /// board-size faces. Board-size faces and glyph pages are kept. The Enhanced tier's
    /// four-sample targets are freed too, and made again by its next frame.
    pub fn low_memory(&mut self) {
        self.waiting.drop_full();
        if let Some(enhanced) = &mut self.enhanced {
            enhanced.low_memory();
        }
        self.backdrop.low_memory();
        if let Some(blur) = &mut self.panel_blur {
            blur.low_memory();
        }
        let full = &self.faces[face_array(FaceSize::Full)];
        // A renderer with no match holds only the token array, which there is no need to free.
        if full.size != dimensions(FaceSize::Full) {
            return;
        }
        self.freed_full = Some(full.format);
        self.faces[face_array(FaceSize::Full)] = token_array(&self.device);
        self.rebind();
    }

    /// Uploads a whole page into its page of the atlas: the shapes of light
    /// (`beryl_assets::light::prepared`), which a host uploads before its first frame, or a
    /// glyph page from the glyph worker. Returns whether it was uploaded: anything but a page,
    /// or a page past `ATLAS_PAGES`, is not. For a glyph page the host then gives the client
    /// the page's `AssetReady`, so that its text draws.
    pub fn upload_page(&mut self, prepared: &Prepared) -> bool {
        let pages = self.atlas.pages();
        let uploaded = self.atlas.upload(&self.device, &self.queue, prepared);
        if self.atlas.pages() != pages {
            self.rebind();
        }
        uploaded
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

    /// Builds the card meshes again at `card`, the frame's card size, when it differs from the
    /// size they were built at, as it does once tuning loaded again changes it. The meshes keep
    /// their number of vertices, so the buffer is rewritten in place.
    pub(crate) fn fit_card_mesh(&mut self, card: Vec2) {
        if card == self.card {
            return;
        }
        let meshes = meshes(card);
        debug_assert_eq!(meshes.ranges, self.ranges);
        self.queue
            .write_buffer(&self.vertices, 0, bytemuck::cast_slice(&meshes.vertices));
        self.card = card;
    }

    /// Draws `frame` into `target`, cleared to black first.
    pub fn render(&mut self, frame: &Frame, target: &mut Target) -> Drawn {
        self.fit_card_mesh(frame.card);
        self.render_view(frame, &target.view, target.width, target.height, None)
    }

    /// The Enhanced tier's lighting `frame` is drawn with, or `None` when it is drawn in
    /// Baseline.
    fn lit_by(&self, frame: &Frame) -> Option<Lighting> {
        (self.enhanced.as_ref())
            .filter(|_| frame.tier == Tier::Enhanced)
            .map(|enhanced| enhanced.tuning)
    }

    /// Draws `frame` into `view`, `width` by `height` physical pixels, cleared to black first, with
    /// the card meshes already built at the frame's card size (`fit_card_mesh`):
    /// in the Enhanced tier when the frame is in it and this renderer can draw it, and in
    /// Baseline otherwise.
    pub(crate) fn render_view(
        &mut self,
        frame: &Frame,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        timing: Option<(&crate::GpuTimer, u64, &mut Option<u32>)>,
    ) -> Drawn {
        let mut drawn = self.render_scene(frame, view, width, height, timing);
        if let Some(inset) = &frame.inset {
            let mut viewport = inset.viewport;
            viewport.x = viewport.x.clamp(0.0, width as f32);
            viewport.y = viewport.y.clamp(0.0, height as f32);
            viewport.width = viewport.width.min(width as f32 - viewport.x);
            viewport.height = viewport.height.min(height as f32 - viewport.y);
            if viewport.width >= 1.0 && viewport.height >= 1.0 {
                let limit = self.device.limits().max_texture_dimension_2d;
                let pixels = |points: f32| (points * inset.scale).round().max(1.0) as u32;
                let width = pixels(inset.size.width).min(limit);
                let height = pixels(inset.size.height).min(limit);
                let target =
                    self.inset
                        .target(&self.device, &self.sampler, self.format, width, height);
                self.fit_card_mesh(inset.frame.card);
                std::mem::swap(&mut self.depth, &mut self.inset.depth);
                let inside = self.render_scene(&inset.frame, &target, width, height, None);
                std::mem::swap(&mut self.depth, &mut self.inset.depth);
                self.inset.paint(&self.device, &self.queue, view, viewport);
                drawn.draw_calls += inside.draw_calls + 1;
            }
        }
        drawn
    }

    #[expect(
        clippy::too_many_lines,
        reason = "the passes and timestamp sample belong to one submission"
    )]
    fn render_scene(
        &mut self,
        frame: &Frame,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        timing: Option<(&crate::GpuTimer, u64, &mut Option<u32>)>,
    ) -> Drawn {
        let lit = self.lit_by(frame);
        let enhanced = lit.is_some();
        let mut uniforms = [0_u8; UNIFORM_BUFFER as usize];
        let written = write_uniforms(frame, width, height, lit.as_ref(), &mut uniforms);
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
            &uniforms[..written as usize],
            records,
        ) else {
            // The device was lost; the host makes a new renderer when it notices.
            return Drawn::default();
        };
        let work = timing
            .as_ref()
            .and_then(|(timer, tick, _)| timer.begin(*tick));
        let draws = Draws {
            frame,
            kept: kept as u32,
            slot,
            vertices: &self.vertices,
            indices: &self.indices,
            ranges: &self.ranges,
            width,
            height,
            timing: work.as_ref(),
        };
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        // With a backdrop filter, the records before it are drawn offscreen, then filtered into
        // `view`, and the rest of the flat layer is drawn over them.
        let backdrop = frame.backdrop.filter(|backdrop| {
            backdrop.at <= draws.kept
                && (backdrop.panel.is_none() || (enhanced && backdrop.at < draws.kept))
        });
        let offscreen = backdrop.map(|backdrop| {
            if let (Some(_), Some(blur)) = (backdrop.panel, &mut self.panel_blur) {
                blur.source(&self.device, [width, height])
            } else {
                self.backdrop.drawn(&self.device, width, height)
            }
        });
        let under = Draws {
            kept: backdrop.map_or(draws.kept, |backdrop| backdrop.at),
            ..draws
        };
        let into = offscreen.as_ref().unwrap_or(view);
        let mut draw_calls = match &mut self.enhanced {
            Some(tier) if enhanced => tier.render(
                &self.device,
                &mut encoder,
                &under,
                into,
                &self.pipelines,
                &depth,
                self.counter.as_ref(),
            ),
            _ => baseline_pass(
                &mut encoder,
                &under,
                into,
                &depth,
                &self.pipelines,
                self.counter.as_ref(),
            ),
        };
        if let Some(backdrop) = backdrop {
            if let (Some(panel), Some(blur)) = (backdrop.panel, &mut self.panel_blur) {
                draw_calls += blur.apply(
                    panel_blur::Apply {
                        device: &self.device,
                        queue: &self.queue,
                        encoder: &mut encoder,
                        source: into,
                        output: view,
                        size: [width, height],
                        counter: self.counter.as_ref(),
                    },
                    &backdrop,
                    panel,
                );
            } else {
                draw_calls += self
                    .backdrop
                    .apply(&self.queue, &mut encoder, &backdrop, view);
            }
            draw_calls += over_pass(
                &mut encoder,
                &draws,
                backdrop.at + u32::from(backdrop.panel.is_some()),
                view,
                &depth,
                &self.pipelines,
                self.counter.as_ref(),
            );
        }
        let measured = timing
            .zip(work)
            .map(|((timer, _, pending), work)| (timer, pending, work));
        self.submit(encoder, measured);
        Drawn { draw_calls }
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

/// Fills the part of the uniform buffer this frame reads and returns its length.
fn write_uniforms(
    frame: &Frame,
    width: u32,
    height: u32,
    lit: Option<&Lighting>,
    uniforms: &mut [u8; UNIFORM_BUFFER as usize],
) -> u64 {
    let mut baseline = [0.0_f32; (UNIFORMS / 4) as usize];
    baseline[..16].copy_from_slice(&frame.view_projection.to_cols_array());
    baseline[16..20].copy_from_slice(&[width as f32, height as f32, frame.shake.x, frame.shake.y]);
    baseline[20] = f32::from_bits(u32::from(frame.paints.masked));
    uniforms[..UNIFORMS as usize].copy_from_slice(bytemuck::cast_slice(&baseline));
    let mut written = UNIFORMS;
    if let Some(tuning) = lit {
        let lit = LIT_OFFSET as usize..LIT_OFFSET as usize + size_of::<Lit>();
        uniforms[lit.clone()].copy_from_slice(bytemuck::bytes_of(&Lit::of(frame, tuning)));
        written = lit.end as u64;
    }
    // Only the part of the paint table the frame uses is written: none when it draws no
    // gradient or mask, whose records then read nothing of it.
    let paints = frame.paints.bytes();
    if !paints.is_empty() {
        let start = PAINT_OFFSET as usize;
        uniforms[start..start + paints.len()].copy_from_slice(paints);
        written = (start + paints.len()) as u64;
    }
    written
}

/// Records the Baseline tier's one pass over every record of `draws` into `encoder`, drawing
/// into `view` with the one-sample `depth` buffer, and returns how many draw calls it took.
/// `counter`, when present, counts the fragments the pass shades.
fn baseline_pass(
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
fn over_pass(
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
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: uniforms,
                        offset: PAINT_OFFSET,
                        size: std::num::NonZeroU64::new(PAINT_TABLE),
                    }),
                },
            ],
        })
    })
}

const _: () = assert!(size_of::<kit_frame::frame::Draw>() as u64 == RECORD);
