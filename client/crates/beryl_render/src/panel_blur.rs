//! Filters a crop of the frame behind a translucent panel in Enhanced. The small targets
//! cover only the panel and the surrounding pixels needed by its blur, and reuse bloom's
//! separable Gaussian. The composite transfers the original picture outside the panel.

use kit_base::geometry::Rect;
use kit_frame::frame::{Backdrop, BackdropPanel};

use crate::count::{self, FragmentCounter};
use crate::enhanced::bloom::{self, Uniforms, WEIGHTS};

const CROP_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const SHADER: &str = concat!(
    include_str!("enhanced/screen.wgsl"),
    include_str!("panel_blur.wgsl")
);

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PanelUniforms {
    crop: [f32; 4],
    rect: [f32; 4],
    clip: [f32; 4],
    appearance: [f32; 4],
    rows: [[f32; 4]; 3],
}

struct Targets {
    size: [u32; 2],
    first: wgpu::TextureView,
    second: wgpu::TextureView,
    across: wgpu::BindGroup,
    down: wgpu::BindGroup,
}

pub(crate) struct PanelBlur {
    blur: wgpu::RenderPipeline,
    downsample: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    blur_layout: wgpu::BindGroupLayout,
    panel_layout: wgpu::BindGroupLayout,
    panel_uniforms: wgpu::Buffer,
    directions: [wgpu::Buffer; 2],
    sampler: wgpu::Sampler,
    format: wgpu::TextureFormat,
    targets: Option<Targets>,
    source: Option<([u32; 2], wgpu::TextureView)>,
}

impl PanelBlur {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let compile = |label, source: &str| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            })
        };
        let blur_shader = compile("blur", bloom::SCREEN_SHADER);
        let panel_shader = compile("panel backdrop", SHADER);
        let blur_layout = bloom::layout(device);
        let panel_layout = panel_layout(device);
        let layout = |group: &wgpu::BindGroupLayout| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("panel backdrop"),
                bind_group_layouts: &[Some(group)],
                immediate_size: 0,
            })
        };
        let panel_pipeline = layout(&panel_layout);
        let blur = bloom::screen_pipeline(
            device,
            &blur_shader,
            &layout(&blur_layout),
            "blur",
            CROP_FORMAT,
            None,
        );
        let downsample = bloom::screen_pipeline(
            device,
            &panel_shader,
            &panel_pipeline,
            "downsample",
            CROP_FORMAT,
            None,
        );
        let composite = bloom::screen_pipeline(
            device,
            &panel_shader,
            &panel_pipeline,
            "composite_panel",
            format,
            None,
        );
        let uniform = |label, size| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        Self {
            blur,
            downsample,
            composite,
            blur_layout,
            panel_layout,
            panel_uniforms: uniform("panel backdrop shape", size_of::<PanelUniforms>() as u64),
            directions: ["blur across", "blur down"]
                .map(|label| uniform(label, size_of::<Uniforms>() as u64)),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            format,
            targets: None,
            source: None,
        }
    }

    pub(crate) fn low_memory(&mut self) {
        self.targets = None;
        self.source = None;
    }

    pub(crate) fn source(&mut self, device: &wgpu::Device, size: [u32; 2]) -> wgpu::TextureView {
        if let Some((held, view)) = &self.source
            && *held == size
        {
            return view.clone();
        }
        let view = texture(device, "before the panel", self.format, size);
        self.source = Some((size, view.clone()));
        view
    }

    pub(crate) fn apply(
        &mut self,
        render: Apply<'_>,
        backdrop: &Backdrop,
        panel: BackdropPanel,
    ) -> u32 {
        let Apply {
            device,
            queue,
            encoder,
            source,
            output,
            size,
            counter,
        } = render;
        let sigma = backdrop.blur.max(0.0);
        // Small blurs retain half resolution. Larger ones use smaller targets so the Gaussian
        // can still reach three deviations on each side with the same maximum number of taps.
        let step = if sigma > 0.0 {
            (3.0 * sigma / (WEIGHTS - 1) as f32).ceil().max(2.0)
        } else {
            1.0
        };
        let (crop, small) = crop(panel.clip, size, (3.0 * sigma).ceil() + step, step);
        let uniforms = PanelUniforms {
            crop,
            rect: components(panel.rect),
            clip: components(panel.clip),
            appearance: [
                panel.radius,
                panel.opacity,
                f32::from(u8::from(self.format.is_srgb())),
                0.0,
            ],
            rows: backdrop.color.map(|[r, g, b]| [r, g, b, 0.0]),
        };
        self.resize(device, small);
        let targets = self.targets.as_ref().expect("crop targets exist");
        queue.write_buffer(&self.panel_uniforms, 0, bytemuck::bytes_of(&uniforms));
        for (axis, buffer) in self.directions.iter().enumerate() {
            let mut direction = [0.0; 2];
            direction[axis] = 1.0;
            let deviation = sigma * small[axis] as f32 / crop[axis + 2];
            let radius = ((3.0 * deviation).ceil() as usize).min(WEIGHTS - 1);
            let blur = Uniforms::blur(direction, deviation, radius);
            queue.write_buffer(buffer, 0, bytemuck::bytes_of(&blur));
        }
        let group = |blurred| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("panel backdrop"),
                layout: &self.panel_layout,
                entries: &[
                    entry(0, self.panel_uniforms.as_entire_binding()),
                    entry(1, wgpu::BindingResource::TextureView(source)),
                    entry(2, wgpu::BindingResource::TextureView(blurred)),
                    entry(3, wgpu::BindingResource::Sampler(&self.sampler)),
                ],
            })
        };
        // The downsample pass cannot bind its own output as a texture.
        let downsample = group(&targets.second);
        let composite = group(&targets.first);
        for (target, pipeline, group) in [
            (&targets.first, &self.downsample, &downsample),
            (&targets.second, &self.blur, &targets.across),
            (&targets.first, &self.blur, &targets.down),
            (output, &self.composite, &composite),
        ] {
            let mut pass = bloom::begin(encoder, "panel backdrop", target, true);
            count::begin(counter, &mut pass);
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
            count::end(counter, &mut pass);
        }
        4
    }

    fn resize(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        if self
            .targets
            .as_ref()
            .is_some_and(|targets| targets.size == size)
        {
            return;
        }
        let first = texture(device, "backdrop crop", CROP_FORMAT, size);
        let second = texture(device, "backdrop crop across", CROP_FORMAT, size);
        let group = |buffer: &wgpu::Buffer, view| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("backdrop blur"),
                layout: &self.blur_layout,
                entries: &[
                    entry(0, buffer.as_entire_binding()),
                    entry(1, wgpu::BindingResource::TextureView(view)),
                    entry(2, wgpu::BindingResource::Sampler(&self.sampler)),
                ],
            })
        };
        let across = group(&self.directions[0], &first);
        let down = group(&self.directions[1], &second);
        self.targets = Some(Targets {
            size,
            first,
            second,
            across,
            down,
        });
    }
}

pub(crate) struct Apply<'a> {
    pub(crate) device: &'a wgpu::Device,
    pub(crate) queue: &'a wgpu::Queue,
    pub(crate) encoder: &'a mut wgpu::CommandEncoder,
    pub(crate) source: &'a wgpu::TextureView,
    pub(crate) output: &'a wgpu::TextureView,
    pub(crate) size: [u32; 2],
    pub(crate) counter: Option<&'a FragmentCounter>,
}

fn crop(clip: Rect, size: [u32; 2], padding: f32, step: f32) -> ([f32; 4], [u32; 2]) {
    let bounds = components(clip);
    let mut crop = [0.0; 4];
    let mut small = [0; 2];
    for axis in 0..2 {
        let end = size[axis] as f32;
        let start = (bounds[axis] - padding).floor().clamp(0.0, end - 1.0);
        let far = (bounds[axis] + bounds[axis + 2] + padding)
            .ceil()
            .clamp(start + 1.0, end);
        crop[axis] = start;
        crop[axis + 2] = far - start;
        small[axis] = ((far - start) / step).ceil() as u32;
    }
    (crop, small)
}

fn texture(
    device: &wgpu::Device,
    label: &str,
    format: wgpu::TextureFormat,
    [width, height]: [u32; 2],
) -> wgpu::TextureView {
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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

fn components(rect: Rect) -> [f32; 4] {
    [rect.x, rect.y, rect.width, rect.height]
}

fn entry(binding: u32, resource: wgpu::BindingResource<'_>) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry { binding, resource }
}

fn panel_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let texture = wgpu::BindingType::Texture {
        sample_type: wgpu::TextureSampleType::Float { filterable: true },
        view_dimension: wgpu::TextureViewDimension::D2,
        multisampled: false,
    };
    let types = [
        wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        texture,
        texture,
        wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
    ];
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("panel backdrop"),
        entries: &std::array::from_fn::<_, 4, _>(|binding| wgpu::BindGroupLayoutEntry {
            binding: binding as u32,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: types[binding],
            count: None,
        }),
    })
}
