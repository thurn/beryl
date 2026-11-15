//! The window: winit 0.30 on the Mac. It draws only when the core says something animates, the
//! pointer moved or a session update arrived, and shows in its title what the last frame cost on
//! the CPU, how many frames were late (missed even a 60 Hz deadline), how many were paced at
//! 60 Hz on a faster display, and whether the AI is searching.

use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::Vec2;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::core::{Core, Input, Wake};
use crate::render::Renderer;
use crate::stats::{Live, drain};
use crate::{Board, Setup, measure};

const BOARDS: [Board; 3] = [Board::Live, Board::Stress(255), Board::Stress(1024)];

#[derive(Default)]
struct Counters {
    frames: u64,
    late: u64,
    /// Frames on time for 60 Hz but not for the display's highest rate.
    slow_paced: u64,
    core_total: f64,
    core_worst: f64,
    encode_total: f64,
    encode_worst: f64,
    allocations_after_first_second: u64,
    last_cpu: f64,
}

struct Shown {
    core: Core,
    /// The clock time at which this core's time began.
    born: Duration,
}

struct State {
    window: Arc<Window>,
    /// The macOS display link that paces frames at the display's highest rate, if one was
    /// made. Without it, frames are requested directly and macOS chooses their rate.
    #[cfg(target_os = "macos")]
    display_link: Option<crate::display_link::DisplayLink>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    boards: [Option<Shown>; 3],
    live: Live,
    current: usize,
    started: Instant,
    paused_at: Option<Instant>,
    paused_total: Duration,
    refresh_hz: f64,
    /// Whether to wait for the GPU after each frame and report how long it took.
    gpu_wait: bool,
    last_gpu_ms: Option<f64>,
    /// When the last frame was presented, the core's time it showed, and when the motion
    /// scheduled at that time ends.
    last_frame: Option<(Instant, u64, u64)>,
    counters: Counters,
    title_at: Instant,
}

struct App {
    setup: Setup,
    proxy: EventLoopProxy<UserEvent>,
    state: Option<State>,
}

/// What other threads and the display link send the event loop.
#[derive(Debug)]
pub enum UserEvent {
    /// The display link fired: draw a frame now.
    Tick,
    /// The session sent an update.
    Update,
}

impl State {
    /// Asks for a frame after an input. With a display link, frames are drawn only on its
    /// ticks: drawing on every mouse move or session update as well submitted frames faster
    /// than the display shows them, used up macOS's few drawables and made the next frame
    /// wait several display intervals for one. The input has reached the core, whose wake now
    /// says a frame is wanted, and the link runs from `about_to_wait`.
    fn request_frame(&self) {
        #[cfg(target_os = "macos")]
        if self.display_link.is_some() {
            return;
        }
        self.window.request_redraw();
    }

    fn clock(&self) -> Duration {
        let end = self.paused_at.unwrap_or_else(Instant::now);
        end.duration_since(self.started)
            .saturating_sub(self.paused_total)
    }

    fn now(&self, index: usize) -> u64 {
        let born = self.boards[index]
            .as_ref()
            .map_or(Duration::ZERO, |b| b.born);
        self.clock().saturating_sub(born).as_nanos() as u64
    }

    fn core(&mut self) -> &mut Core {
        &mut self.boards[self.current]
            .as_mut()
            .expect("the shown board exists")
            .core
    }

    fn redraw(&mut self) {
        if let Some(Shown { core, .. }) = &mut self.boards[0] {
            drain(core, Some(&self.live));
        }
        let acquiring = Instant::now();
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.renderer.device, &self.config);
                return;
            }
            _ => return,
        };
        let acquire_ns = acquiring.elapsed().as_nanos() as f64;
        // The time is read after the drawable is acquired, which can wait for the display, so
        // the frame shows the moment it is drawn for.
        let now = self.now(self.current);
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let t0 = Instant::now();
        let (stats, allocations) = measure::count_allocations(|| self.core().frame(now).stats);
        let core_ns = t0.elapsed().as_nanos() as f64;
        let (width, height) = (self.config.width, self.config.height);
        let t1 = Instant::now();
        let shown = &self.boards[self.current].as_ref().expect("shown").core;
        self.renderer.draw(
            &view,
            width,
            height,
            shown.camera.view_proj,
            &shown.draw.records,
            stats.opaque,
            false,
        );
        let encode_ns = t1.elapsed().as_nanos() as f64;
        self.window.pre_present_notify();
        self.renderer.queue.present(frame);
        // With BERYL3D_GPU_WAIT set, the frame thread waits for the GPU to finish each frame
        // and the late-frame lines say how long that took. If the GPU finishes quickly while
        // the wait for a drawable stays long, the compositor is what holds the frames back;
        // if the GPU itself is slow, other work on the GPU is.
        let gpu_ms = if self.gpu_wait {
            self.renderer.wait_idle();
            Some(t1.elapsed().as_secs_f64() * 1e3)
        } else {
            None
        };
        let previous_gpu_ms = std::mem::replace(&mut self.last_gpu_ms, gpu_ms);
        // A gap between two frames is late only for as long as motion was scheduled through
        // it: after the last segment ends, the table is still and needs no frame, however
        // long it stays still. `due` is how long motion went on after the previous frame.
        let motion_until = shown.motion_until();
        let at = Instant::now();
        if let Some((previous, previous_now, previous_until)) = self.last_frame {
            let interval = at.duration_since(previous).as_secs_f64();
            let due = previous_until.saturating_sub(previous_now) as f64 / 1e9;
            if interval > 1.5 / 60.0 && due > 1.5 / 60.0 {
                // Late even for a 60 Hz display: a frame was really dropped. One line each,
                // saying where its time went.
                self.counters.late += 1;
                let gpu = match (previous_gpu_ms, gpu_ms) {
                    (Some(before), Some(now)) => {
                        format!("; GPU done {before:.2} ms after submit before, {now:.2} ms now")
                    }
                    _ => String::new(),
                };
                eprintln!(
                    "late frame {}: {:.1} ms since the last; waited {:.2} ms for the drawable, \
                     core {:.3} ms, encode and submit {:.3} ms; {} updates waiting; load {:.1}{gpu}",
                    self.counters.late,
                    interval * 1e3,
                    acquire_ns / 1e6,
                    core_ns / 1e6,
                    encode_ns / 1e6,
                    shown.queued(),
                    measure::load_average(),
                );
            } else if interval > 1.5 / self.refresh_hz && due > 1.5 / self.refresh_hz {
                // On time for 60 Hz but not for the display's highest rate: macOS paced the
                // window at a lower rate, which it does unless the app asks for more through a
                // display link, as winit's on-demand redraws do not.
                self.counters.slow_paced += 1;
            }
        }
        self.last_frame = Some((at, now, motion_until));
        let c = &mut self.counters;
        c.frames += 1;
        c.core_total += core_ns;
        c.encode_total += encode_ns;
        c.core_worst = c.core_worst.max(core_ns);
        c.encode_worst = c.encode_worst.max(encode_ns);
        c.last_cpu = core_ns + encode_ns;
        if now > 1_000_000_000 {
            c.allocations_after_first_second += allocations;
        }
        if at.duration_since(self.title_at) > Duration::from_millis(200) {
            self.title_at = at;
            self.retitle();
        }
    }

    fn retitle(&self) {
        let searching = self.boards[0].as_ref().is_some_and(|b| b.core.thinking);
        let title = format!(
            "Beryl 3D prototype · {} · CPU {:.3} ms · late {} · at 60 Hz {} · AI {}{}",
            BOARDS[self.current].name(),
            self.counters.last_cpu / 1e6,
            self.counters.late,
            self.counters.slow_paced,
            if searching { "searching" } else { "idle" },
            if self.paused_at.is_some() {
                " · paused"
            } else {
                ""
            },
        );
        self.window.set_title(&title);
    }

    fn print_stats(&self) {
        let c = &self.counters;
        let n = c.frames.max(1) as f64;
        let shown = &self.boards[self.current].as_ref().expect("shown").core;
        let s = shown.draw.stats;
        println!("--- {} ---", BOARDS[self.current].name());
        println!(
            "frames drawn {}  late {}  at 60 Hz {}  display {:.0} Hz  load {:.1}",
            c.frames,
            c.late,
            c.slow_paced,
            self.refresh_hz,
            measure::load_average()
        );
        println!(
            "core per frame: mean {:.0} ns, worst {:.0} ns",
            c.core_total / n,
            c.core_worst
        );
        println!(
            "encode and submit: mean {:.0} ns, worst {:.0} ns",
            c.encode_total / n,
            c.encode_worst
        );
        println!(
            "core allocations after the first second: {}",
            c.allocations_after_first_second
        );
        println!(
            "last frame: {} records ({} opaque, {} blended), {} draw calls, opaque area {:.2}, blended area {:.2} screens",
            s.records, s.opaque, s.blended, s.draw_calls, s.opaque_area, s.blended_area
        );
        println!(
            "updates waiting for their turn: {}",
            self.boards[0].as_ref().map_or(0, |b| b.core.queued())
        );
        println!("peak memory {:.1} MB", measure::peak_memory() as f64 / 1e6);
    }

    fn wake(&self) -> Wake {
        if self.paused_at.is_some() {
            return Wake::Never;
        }
        let shown = &self.boards[self.current].as_ref().expect("shown").core;
        shown.wake(self.now(self.current))
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("Beryl 3D prototype")
            .with_inner_size(winit::dpi::LogicalSize::new(1440.0, 900.0));
        let window = Arc::new(event_loop.create_window(attributes).expect("a window"));
        let refresh_hz = window
            .current_monitor()
            .map(|m| {
                m.video_modes()
                    .filter_map(|v| Some(v.refresh_rate_millihertz()))
                    .max()
                    .or(m.refresh_rate_millihertz())
                    .unwrap_or(60_000)
            })
            .unwrap_or(60_000) as f64
            / 1000.0;
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window.clone()).expect("a surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            ..Default::default()
        }))
        .expect("an adapter");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .expect("a device");
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .expect("the surface works with the adapter");
        let capabilities = surface.get_capabilities(&adapter);
        if let Some(format) = capabilities.formats.iter().find(|f| f.is_srgb()) {
            config.format = *format;
        }
        config.present_mode = wgpu::PresentMode::Fifo;
        // Two frames in flight, wgpu's default. With one, every frame waited for the compositor
        // to give back the previous drawable before it could start.
        config.desired_maximum_frame_latency = 2;
        surface.configure(&device, &config);
        let renderer = Renderer::new(device, queue, config.format, &self.setup.faces, false);
        let proxy = self.proxy.clone();
        let (sessions, id, updates) = crate::game::start(move || {
            let _ = proxy.send_event(UserEvent::Update);
        });
        let live = Live {
            sessions,
            id,
            updates,
        };
        let core = Core::new(
            config.width,
            config.height,
            self.setup.timing,
            self.setup.faces.faces.clone(),
        );
        println!(
            "display: up to {refresh_hz:.0} Hz; faces: {} scans, {} placeholders",
            self.setup.faces.scans, self.setup.faces.missing
        );
        #[cfg(target_os = "macos")]
        let display_link = if std::env::var_os("BERYL3D_NO_DISPLAY_LINK").is_some() {
            None
        } else {
            crate::display_link::DisplayLink::new(&window, self.proxy.clone(), refresh_hz as f32)
        };
        #[cfg(target_os = "macos")]
        println!(
            "display link: {}",
            if display_link.is_some() {
                "on (set BERYL3D_NO_DISPLAY_LINK=1 to compare)"
            } else {
                "off"
            }
        );
        println!(
            "keys: 1 live game, 2 stress-255, 3 stress-1024, Space pause, F fullscreen, S statistics"
        );
        self.state = Some(State {
            window,
            #[cfg(target_os = "macos")]
            display_link,
            surface,
            config,
            renderer,
            boards: [
                Some(Shown {
                    core,
                    born: Duration::ZERO,
                }),
                None,
                None,
            ],
            live,
            current: 0,
            started: Instant::now(),
            paused_at: None,
            paused_total: Duration::ZERO,
            refresh_hz,
            last_frame: None,
            gpu_wait: std::env::var_os("BERYL3D_GPU_WAIT").is_some(),
            last_gpu_ms: None,
            counters: Counters::default(),
            title_at: Instant::now(),
        });
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        let Some(state) = &mut self.state else {
            return;
        };
        match event {
            UserEvent::Tick => state.window.request_redraw(),
            UserEvent::Update => {
                // The update goes into the core now, so that the core's wake sees it and the
                // display link starts; the frame itself waits for the next tick.
                if let Some(Shown { core, .. }) = &mut state.boards[0] {
                    drain(core, Some(&state.live));
                }
                state.request_frame();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(state) = &mut self.state else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                state.config.width = size.width.max(1);
                state.config.height = size.height.max(1);
                state
                    .surface
                    .configure(&state.renderer.device, &state.config);
                for shown in state.boards.iter_mut().flatten() {
                    shown.core.handle(Input::Surface {
                        width: state.config.width,
                        height: state.config.height,
                    });
                }
                state.window.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                let ndc = Vec2::new(
                    position.x as f32 / state.config.width as f32 * 2.0 - 1.0,
                    1.0 - position.y as f32 / state.config.height as f32 * 2.0,
                );
                state.core().handle(Input::Pointer(Some(ndc)));
                state.request_frame();
            }
            WindowEvent::CursorLeft { .. } => {
                state.core().handle(Input::Pointer(None));
                state.request_frame();
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed && !event.repeat =>
            {
                match event.logical_key.as_ref() {
                    Key::Character("1") => state.current = 0,
                    Key::Character(c @ ("2" | "3")) => {
                        let index = if c == "2" { 1 } else { 2 };
                        if state.boards[index].is_none() {
                            let Board::Stress(cards) = BOARDS[index] else {
                                unreachable!()
                            };
                            let core = Core::stress(
                                state.config.width,
                                state.config.height,
                                self.setup.timing,
                                self.setup.faces.faces.clone(),
                                cards,
                            );
                            state.boards[index] = Some(Shown {
                                core,
                                born: state.clock(),
                            });
                        }
                        state.current = index;
                    }
                    Key::Named(NamedKey::Space) => match state.paused_at.take() {
                        Some(at) => state.paused_total += at.elapsed(),
                        None => state.paused_at = Some(Instant::now()),
                    },
                    Key::Character("f" | "F") => {
                        // Fullscreen, where macOS can show a Metal layer without composing
                        // it with other windows, to tell compositor stalls from our own.
                        let full = state.window.fullscreen().is_none();
                        state.window.set_fullscreen(
                            full.then_some(winit::window::Fullscreen::Borderless(None)),
                        );
                        return;
                    }
                    Key::Character("s" | "S") => {
                        state.print_stats();
                        return;
                    }
                    _ => return,
                }
                state.counters = Counters::default();
                state.last_frame = None;
                state.retitle();
                state.window.request_redraw();
            }
            WindowEvent::RedrawRequested => state.redraw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(state) = &mut self.state else {
            return;
        };
        let wake = state.wake();
        // With a display link, its ticks request the frames while the core wants them, and it
        // is paused otherwise. Without one, each frame asks for the next.
        #[cfg(target_os = "macos")]
        let paced = match &mut state.display_link {
            Some(link) => {
                link.set_running(wake == Wake::Now);
                true
            }
            None => false,
        };
        #[cfg(not(target_os = "macos"))]
        let paced = false;
        match wake {
            Wake::Now => {
                if !paced {
                    state.window.request_redraw();
                }
                event_loop.set_control_flow(ControlFlow::Wait);
            }
            Wake::At(at) => {
                let now = state.now(state.current);
                let wait = Duration::from_nanos(at.saturating_sub(now));
                event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + wait));
            }
            Wake::Never => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }
}

pub fn run(setup: Setup) {
    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .expect("an event loop");
    let proxy = event_loop.create_proxy();
    let mut app = App {
        setup,
        proxy,
        state: None,
    };
    event_loop.run_app(&mut app).expect("the event loop runs");
}
