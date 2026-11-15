//! The window: winit 0.30 on the Mac. It draws only when the core says something animates, the
//! pointer moved or a session update arrived, and shows in its title what the last frame cost on
//! the CPU, how many frames were late and whether the AI is searching.

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
    /// When the last frame was presented, and whether the core asked for another at once.
    last_frame: Option<(Instant, bool)>,
    counters: Counters,
    title_at: Instant,
}

struct App {
    setup: Setup,
    proxy: EventLoopProxy<()>,
    state: Option<State>,
}

impl State {
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
        let animating = shown.animating(now);
        // Whether the core wants the next frame straight away. Only then is a long gap before
        // the next frame late: after a still table, the first frame that an input such as the
        // pointer starts is not, however long the table was still.
        let continues = shown.wake(self.now(self.current)) == Wake::Now;
        let at = Instant::now();
        if let Some((previous, expected)) = self.last_frame
            && expected
            && animating
            && at.duration_since(previous).as_secs_f64() > 1.5 / self.refresh_hz
        {
            self.counters.late += 1;
            // One line per late frame, saying where its time went, so a person running the
            // window can tell drawing cost from presentation and scheduling.
            eprintln!(
                "late frame {}: {:.1} ms since the last (display {:.1} ms); waited {:.2} ms for \
                 the drawable, core {:.3} ms, encode and submit {:.3} ms; {} updates waiting; \
                 load {:.1}",
                self.counters.late,
                at.duration_since(previous).as_secs_f64() * 1e3,
                1e3 / self.refresh_hz,
                acquire_ns / 1e6,
                core_ns / 1e6,
                encode_ns / 1e6,
                shown.queued(),
                measure::load_average(),
            );
        }
        self.last_frame = Some((at, continues));
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
            "Beryl 3D prototype · {} · CPU {:.3} ms · late {} · AI {}{}",
            BOARDS[self.current].name(),
            self.counters.last_cpu / 1e6,
            self.counters.late,
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
            "frames drawn {}  late {}  display {:.0} Hz  load {:.1}",
            c.frames,
            c.late,
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

impl ApplicationHandler<()> for App {
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
            let _ = proxy.send_event(());
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
        println!("keys: 1 live game, 2 stress-255, 3 stress-1024, Space pause, S statistics");
        self.state = Some(State {
            window,
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
            counters: Counters::default(),
            title_at: Instant::now(),
        });
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, (): ()) {
        if let Some(state) = &self.state {
            state.window.request_redraw();
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
                state.window.request_redraw();
            }
            WindowEvent::CursorLeft { .. } => {
                state.core().handle(Input::Pointer(None));
                state.window.request_redraw();
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
        let Some(state) = &self.state else {
            return;
        };
        match state.wake() {
            Wake::Now => {
                state.window.request_redraw();
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
    let event_loop = EventLoop::<()>::with_user_event()
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
