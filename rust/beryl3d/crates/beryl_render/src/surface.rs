//! A window's surface: the drawables a host presents frames in (docs/beryl-3d.md, "Hosts").
//! The host passes its window; no wgpu type crosses into the host's code.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::time::{Duration, Instant};

use kit_base::data::lighting::Lighting;
use kit_frame::frame::Frame;

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

/// A phone's native drawing surface, as its host passes it: a raw pointer the host owns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Native {
    /// A `CAMetalLayer`, on iOS.
    MetalLayer(NonNull<c_void>),
    /// An `ANativeWindow`, on Android.
    AndroidWindow(NonNull<c_void>),
}

impl Renderer {
    /// A renderer on the GPU adapter that can present to `window`, and the window's surface,
    /// `width` by `height` physical pixels. The device asks for wgpu's limits for OpenGL ES 3.0,
    /// as a phone's does, with the adapter's own texture size: the floor's 2013 integrated GPUs
    /// may lack wgpu's default limits, and a desktop run that outgrew the floor would fail here
    /// rather than on the floor. `lighting` is as for [`Renderer::new`].
    pub fn for_window<W>(
        window: W,
        width: u32,
        height: u32,
        lighting: Lighting,
    ) -> Result<(Self, Surface), String>
    where
        W: wgpu::WindowHandle + wgpu::rwh::HasDisplayHandle + 'static,
    {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance
            .create_surface(window)
            .map_err(|error| format!("no surface for the window: {error}"))?;
        Self::on_surface(
            instance,
            surface,
            width,
            height,
            wgpu::Limits::downlevel_webgl2_defaults(),
            lighting,
        )
    }

    /// A renderer on the GPU adapter that can present to a phone's `native` surface, and that
    /// surface, `width` by `height` physical pixels. The device is created with wgpu's limits
    /// for OpenGL ES 3.0, the floor's, so that the same device works on Metal, Vulkan and
    /// OpenGL ES. `lighting` is as for [`Renderer::new`].
    ///
    /// # Safety
    ///
    /// `native` must point to a live layer or window of its kind, and stay valid until the
    /// returned surface is dropped.
    #[allow(unsafe_code)]
    pub unsafe fn for_native(
        native: Native,
        width: u32,
        height: u32,
        lighting: Lighting,
    ) -> Result<(Self, Surface), String> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        // No debug names on GPU objects. The Android emulator's Vulkan driver
        // (vulkan.ranchu.so) crashed with SIGSEGV in vkSetDebugUtilsObjectNameEXT while wgpu
        // named the device's first objects, and a debug build names every object. Validation
        // stays on.
        descriptor.flags.remove(wgpu::InstanceFlags::DEBUG);
        descriptor.backends = native_backend(native);
        let instance = wgpu::Instance::new(descriptor);
        // SAFETY: the caller's promise, passed on.
        let surface = unsafe { create_native(&instance, native) }?;
        Self::on_surface(
            instance,
            surface,
            width,
            height,
            wgpu::Limits::downlevel_webgl2_defaults(),
            lighting,
        )
    }

    /// A new surface on this renderer's GPU, for the native surface a phone host supplies after
    /// it lost the last one (the app went to the background and came back).
    ///
    /// # Safety
    ///
    /// As for [`Renderer::for_native`].
    #[allow(unsafe_code)]
    pub unsafe fn attach(
        &self,
        native: Native,
        width: u32,
        height: u32,
    ) -> Result<Surface, String> {
        let (instance, adapter) = self
            .gpu
            .as_ref()
            .ok_or("this renderer draws offscreen and has no surface")?;
        // SAFETY: the caller's promise, passed on.
        let surface = unsafe { create_native(instance, native) }?;
        configure(&self.device, adapter, surface, width, height)
    }

    /// A renderer on the GPU adapter that can present to `surface`, with `limits`, lit by
    /// `lighting`.
    fn on_surface(
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
        limits: wgpu::Limits,
        lighting: Lighting,
    ) -> Result<(Self, Surface), String> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .map_err(|error| format!("no GPU adapter for the window: {error}"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("beryl_render"),
            // Card faces are compressed in the first format of these the GPU has.
            // Frames write their buffers directly where that saves the driver's blit pool.
            required_features: (adapter.features() & crate::faces::COMPRESSION)
                | crate::slots::mapped_features(&adapter),
            required_limits: limits.using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .map_err(|error| format!("no GPU device: {error}"))?;
        let surface = configure(&device, &adapter, surface, width, height)?;
        let mut renderer = Self::new(device, queue, surface.config.format, lighting);
        let info = adapter.get_info();
        renderer.adapter = format!("{} ({:?})", info.name, info.backend);
        renderer.gpu = Some((instance, adapter));
        Ok((renderer, surface))
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

    /// Draws `frame` again, as `present` just did, into a texture the size of `surface` and
    /// starts reading it back. The window's own drawable is gone by then, so a picture of what
    /// the window shows is a second drawing of the same frame.
    pub fn read_back_frame(&mut self, frame: &Frame, surface: &Surface) -> crate::Readback {
        let mut target =
            crate::Target::offscreen(self, surface.config.width, surface.config.height);
        self.render(frame, &mut target);
        self.read_back(&target)
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

/// Configures `surface` for drawing on `device`.
fn configure(
    device: &wgpu::Device,
    adapter: &wgpu::Adapter,
    surface: wgpu::Surface<'static>,
    width: u32,
    height: u32,
) -> Result<Surface, String> {
    let mut config = surface
        .get_default_config(adapter, width.max(1), height.max(1))
        .ok_or("the window's surface does not work with the GPU adapter")?;
    // Colors are sRGB values blended as they are stored, as the offscreen target does, so the
    // surface takes a format that does no conversion.
    let capabilities = surface.get_capabilities(adapter);
    if let Some(format) = capabilities.formats.iter().find(|format| !format.is_srgb()) {
        config.format = *format;
    }
    // Paced by the display: the host draws on the display's ticks and never faster.
    config.present_mode = wgpu::PresentMode::Fifo;
    config.desired_maximum_frame_latency = FRAMES_IN_FLIGHT;
    surface.configure(device, &config);
    Ok(Surface { surface, config })
}

/// The one backend a phone's renderer uses: Metal on iOS; on Android Vulkan where the device
/// has a Vulkan adapter, and OpenGL ES 3.0 where it has none. Chosen before any surface exists,
/// because an instance with both makes a surface for each, and Android's Vulkan loader connects
/// the window to the EGL API when it makes its surface, after which EGL's
/// `eglCreateWindowSurface` fails with `EGL_BAD_ALLOC`.
fn native_backend(native: Native) -> wgpu::Backends {
    match native {
        Native::MetalLayer(_) => wgpu::Backends::METAL,
        Native::AndroidWindow(_) => {
            let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
            descriptor.backends = wgpu::Backends::VULKAN;
            let vulkan = wgpu::Instance::new(descriptor);
            if pollster::block_on(vulkan.enumerate_adapters(wgpu::Backends::VULKAN)).is_empty() {
                wgpu::Backends::GL
            } else {
                wgpu::Backends::VULKAN
            }
        }
    }
}

/// A wgpu surface on a phone's native surface.
///
/// # Safety
///
/// As for [`Renderer::for_native`].
#[allow(unsafe_code)]
unsafe fn create_native(
    instance: &wgpu::Instance,
    native: Native,
) -> Result<wgpu::Surface<'static>, String> {
    use wgpu::rwh::{
        AndroidDisplayHandle, AndroidNdkWindowHandle, RawDisplayHandle, RawWindowHandle,
    };
    let target = match native {
        // wgpu draws into the layer itself: the iOS host's view is backed by it.
        #[cfg(target_vendor = "apple")]
        Native::MetalLayer(layer) => wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(layer.as_ptr()),
        #[cfg(not(target_vendor = "apple"))]
        Native::MetalLayer(_) => return Err("a Metal layer needs an Apple platform".to_owned()),
        Native::AndroidWindow(window) => wgpu::SurfaceTargetUnsafe::RawHandle {
            // wgpu's OpenGL ES backend refuses a window with no display, even on Android,
            // whose display handle carries nothing.
            raw_display_handle: Some(RawDisplayHandle::Android(AndroidDisplayHandle::new())),
            raw_window_handle: RawWindowHandle::AndroidNdk(AndroidNdkWindowHandle::new(window)),
        },
    };
    // SAFETY: wgpu's `create_surface_unsafe` needs the layer or window to stay valid until the
    // surface is dropped, which the caller promises.
    unsafe { instance.create_surface_unsafe(target) }
        .map_err(|error| format!("no surface for the native {native:?}: {error}"))
}
