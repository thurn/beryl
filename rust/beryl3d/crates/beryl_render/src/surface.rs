//! A window's surface: the drawables a host presents frames in (docs/beryl-3d.md, "Hosts").
//! The host passes its window; no wgpu type crosses into the host's code.

use std::time::{Duration, Instant};

use beryl_scene::frame::Frame;

use crate::renderer::{Drawn, Renderer};

/// How many frames may wait for the display at once. With one, every frame waited for the
/// compositor to hand back the previous drawable before it could start.
const FRAMES_IN_FLIGHT: u32 = 2;

/// A window's drawing surface, `width` by `height` physical pixels.
pub struct Surface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

/// What became of one call to `Renderer::present`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presented {
    /// The frame was drawn and handed to the display, after waiting `waited` for a drawable.
    Shown { drawn: Drawn, waited: Duration },
    /// No drawable was available (the window is hidden, or the surface was out of date and has
    /// been configured again), so nothing was drawn. The next frame tries again.
    Skipped,
}

impl Renderer {
    /// A renderer on the GPU adapter that can present to `window`, and the window's surface,
    /// `width` by `height` physical pixels.
    pub fn for_window<W>(window: W, width: u32, height: u32) -> Result<(Self, Surface), String>
    where
        W: wgpu::WindowHandle + wgpu::rwh::HasDisplayHandle + 'static,
    {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance
            .create_surface(window)
            .map_err(|error| format!("no surface for the window: {error}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .map_err(|error| format!("no GPU adapter for the window: {error}"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("beryl_render"),
            ..Default::default()
        }))
        .map_err(|error| format!("no GPU device: {error}"))?;
        let mut config = surface
            .get_default_config(&adapter, width.max(1), height.max(1))
            .ok_or("the window's surface does not work with the GPU adapter")?;
        // Colors are sRGB values blended as they are stored, as the offscreen target does, so
        // the surface takes a format that does no conversion.
        let capabilities = surface.get_capabilities(&adapter);
        if let Some(format) = capabilities.formats.iter().find(|format| !format.is_srgb()) {
            config.format = *format;
        }
        // Paced by the display: the host draws on the display's ticks and never faster.
        config.present_mode = wgpu::PresentMode::Fifo;
        config.desired_maximum_frame_latency = FRAMES_IN_FLIGHT;
        surface.configure(&device, &config);
        let mut renderer = Self::new(device, queue, config.format);
        renderer.adapter = adapter.get_info().name;
        Ok((renderer, Surface { surface, config }))
    }

    /// Draws `frame` into the surface's next drawable and hands it to the display. `before`
    /// runs just before the drawable is presented (winit's `pre_present_notify`).
    pub fn present(
        &mut self,
        frame: &Frame,
        surface: &mut Surface,
        before: impl FnOnce(),
    ) -> Presented {
        let asked = Instant::now();
        let drawable = match surface.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(drawable)
            | wgpu::CurrentSurfaceTexture::Suboptimal(drawable) => drawable,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                surface.surface.configure(&self.device, &surface.config);
                return Presented::Skipped;
            }
            _ => return Presented::Skipped,
        };
        let waited = asked.elapsed();
        let view = drawable
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let drawn = self.render_view(frame, &view, surface.config.width, surface.config.height);
        before();
        self.queue.present(drawable);
        Presented::Shown { drawn, waited }
    }

    /// Changes the surface's size to `width` by `height` physical pixels.
    pub fn resize(&self, surface: &mut Surface, width: u32, height: u32) {
        let (width, height) = (width.max(1), height.max(1));
        if (surface.config.width, surface.config.height) != (width, height) {
            surface.config.width = width;
            surface.config.height = height;
            surface.surface.configure(&self.device, &surface.config);
        }
    }
}
