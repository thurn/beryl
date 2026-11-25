//! The desktop host on winit 0.30 (docs/beryl-3d.md, "Hosts"): it opens the window and its
//! surface, turns winit's events into `Input`s, draws on the display's ticks and carries out the
//! client's `Request`s. On macOS a display link supplies the ticks; elsewhere winit's redraw
//! requests do.

use kit_ui::input::Input as DeviceInput;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use app_client::Client;
use app_client::Input;
use app_client::command::Request;
use beryl_render::Renderer;
use kit_ui::input::Lifecycle;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::assets::Assets;
use crate::clipboard::Clipboard;
use crate::events::Translator;
use crate::frames::{Chosen, Feed, FrameLoop, Opened, Opening, Sleep, Ticks};
use crate::inbox::Inbox;
use crate::timing::{DisplayTick, FrameTiming, Outcome, Timing, elapsed};
use crate::winit_screen::Screen;

/// What the window is (decision C15: the faster choice is the default).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowMode {
    /// A window that opens behind the other windows and never takes focus or keyboard input,
    /// for runs an agent starts while a person works.
    Background,
    /// An ordinary window.
    Foreground,
    /// The whole display. A key leaves and re-enters it (`is_fullscreen_key`). On macOS it is
    /// a borderless window over the whole display that the window server composes as usual
    /// (`set_fullscreen`), not a fullscreen Space.
    Fullscreen,
}

/// How the desktop host opens its window.
#[derive(Clone, Debug)]
pub struct Options {
    pub mode: WindowMode,
    pub title: String,
    /// The window's size in logical points when it is not fullscreen.
    pub size: (f64, f64),
    /// Where the client's clock stops, in nanoseconds since it started.
    pub pause_at: Option<u64>,
    /// How long the host runs before it ends by itself, if it does.
    pub end_after: Option<Duration>,
    /// How long each finished frame is held back before it is shown, to imitate a slow GPU
    /// (`--slow-gpu`). Zero shows it at once.
    pub slow_gpu: Duration,
}

/// What other threads and the display link send the event loop.
#[derive(Debug)]
pub enum UserEvent {
    /// The display link fired: the display is ready for a frame.
    #[cfg(target_os = "macos")]
    Tick(DisplayTick),
    /// Inputs are waiting in the inbox.
    Inbox,
}

/// The desktop host before it runs.
pub struct WinitHost {
    event_loop: EventLoop<UserEvent>,
    options: Options,
    sender: Sender<Input>,
    receiver: Receiver<Input>,
    #[cfg(feature = "drive")]
    drive: Option<crate::drive::Drive>,
}

impl WinitHost {
    pub fn new(options: Options) -> Result<Self, String> {
        let mut builder = EventLoop::<UserEvent>::with_user_event();
        #[cfg(target_os = "macos")]
        if options.mode == WindowMode::Background {
            use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
            // An accessory app has no Dock icon and is never made the active app, so its
            // window stays behind the active app's windows and never gets the keyboard.
            builder
                .with_activation_policy(ActivationPolicy::Accessory)
                .with_activate_ignoring_other_apps(false);
        }
        let event_loop = builder
            .build()
            .map_err(|error| format!("no event loop: {error}"))?;
        let (sender, receiver) = channel();
        Ok(Self {
            event_loop,
            options,
            sender,
            receiver,
            #[cfg(feature = "drive")]
            drive: None,
        })
    }

    /// The same host playing `drive` on the display's clock, ending the run after its last
    /// action.
    #[cfg(feature = "drive")]
    #[must_use]
    pub fn driven(mut self, drive: Option<crate::drive::Drive>) -> Self {
        self.drive = drive;
        self
    }

    /// A handle other threads use to send inputs.
    pub fn inbox(&self) -> Inbox {
        let proxy = self.event_loop.create_proxy();
        Inbox::new(self.sender.clone(), move || {
            proxy.send_event(UserEvent::Inbox).is_ok()
        })
    }

    /// Runs `client` in the window until it is closed, `Options::end_after` has passed or the
    /// drive script or the feed has ended (`FrameLoop::at_end`), with `feed` added to its frame loop. `setup` prepares the renderer (its card faces, say)
    /// before the first frame, and chooses the tier the client starts in.
    pub fn run(
        self,
        client: Client,
        feed: Box<dyn Feed>,
        setup: impl FnOnce(&mut Renderer, Opening) -> Chosen,
    ) -> Result<(), String> {
        let wake = self.inbox();
        let mut app = App {
            wake,
            options: self.options,
            #[cfg(target_os = "macos")]
            proxy: self.event_loop.create_proxy(),
            inbox: self.receiver,
            client: Some((client, feed)),
            #[cfg(feature = "drive")]
            drive: self.drive,
            setup: Some(setup),
            running: None,
            started: Instant::now(),
            error: None,
        };
        self.event_loop
            .run_app(&mut app)
            .map_err(|error| format!("the event loop failed: {error}"))?;
        app.error.map_or(Ok(()), Err)
    }
}

/// Whether a key press leaves or re-enters fullscreen: F11, or ⌃⌘F on macOS. The client never
/// sees it.
pub fn is_fullscreen_key(key: PhysicalKey, control: bool, command: bool) -> bool {
    match key {
        PhysicalKey::Code(KeyCode::F11) => true,
        PhysicalKey::Code(KeyCode::KeyF) => cfg!(target_os = "macos") && control && command,
        _ => false,
    }
}

struct App<S> {
    options: Options,
    /// Wakes the event loop when an asset worker finishes a delivery.
    wake: Inbox,
    /// Where the display link sends its ticks.
    #[cfg(target_os = "macos")]
    proxy: winit::event_loop::EventLoopProxy<UserEvent>,
    inbox: Receiver<Input>,
    /// The client and the app's feed until the window opens, when they move into `running`.
    client: Option<(Client, Box<dyn Feed>)>,
    /// The drive script until the window opens, when it moves into the frame loop.
    #[cfg(feature = "drive")]
    drive: Option<crate::drive::Drive>,
    setup: Option<S>,
    running: Option<Running>,
    /// The client's time 0.
    started: Instant,
    error: Option<String>,
}

/// The host once its window is open.
struct Running {
    window: Arc<Window>,
    frames: FrameLoop<Pacer>,
    screen: Screen,
    translator: Translator,
    clipboard: Clipboard,
    fullscreen: bool,
}

/// Where ticks come from.
enum Pacer {
    /// A macOS display link: its ticks arrive as `UserEvent::Tick`.
    #[cfg(target_os = "macos")]
    Link(crate::macos::DisplayLink),
    /// winit's redraw requests: each `RedrawRequested` is a tick, and while frames are wanted
    /// every pass of the event loop asks for the next.
    Redraw(Arc<Window>),
}

impl Ticks for Pacer {
    fn set_running(&mut self, running: bool) {
        match self {
            #[cfg(target_os = "macos")]
            Pacer::Link(link) => link.set_running(running),
            Pacer::Redraw(window) => {
                if running {
                    window.request_redraw();
                }
            }
        }
    }

    fn set_rate(&mut self, hz: f32) {
        #[cfg(target_os = "macos")]
        if let Pacer::Link(link) = self {
            link.set_rate(hz);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = hz;
    }
}

impl<S: FnOnce(&mut Renderer, Opening) -> Chosen> App<S> {
    fn now(&self) -> u64 {
        self.started.elapsed().as_nanos() as u64
    }

    fn open(&mut self, event_loop: &ActiveEventLoop) -> Result<Running, String> {
        let (width, height) = self.options.size;
        let mut attributes = Window::default_attributes()
            .with_title(self.options.title.clone())
            .with_inner_size(LogicalSize::new(width, height));
        if self.options.mode == WindowMode::Background {
            attributes = attributes.with_active(false);
        }
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|error| format!("no window: {error}"))?,
        );
        if self.options.mode == WindowMode::Fullscreen {
            set_fullscreen(&window, true);
        }
        let size = window.inner_size();
        let lighting = (self.client.as_ref())
            .map(|(client, _)| client.data().lighting)
            .ok_or("the window opened twice")?;
        let (mut renderer, surface) =
            Renderer::for_window(Arc::clone(&window), size.width, size.height, lighting)?;
        let assets = Assets::live({
            let wake = self.wake.clone();
            move || {
                wake.send(Input::Nudge);
            }
        });
        let display_hz = self.display_hz(&window);
        let chosen = self.setup.take().map_or(Chosen::BASELINE, |setup| {
            setup(
                &mut renderer,
                Opening {
                    width: size.width,
                    height: size.height,
                    scale: window.scale_factor() as f32,
                    display_hz,
                },
            )
        });
        let (client, mut feed) = self.client.take().ok_or("the window opened twice")?;
        let translator = Translator::new(size, window.scale_factor());
        crate::threads::interactive_thread();
        feed.opened(&Opened {
            display_hz,
            adapter: renderer.adapter().to_owned(),
        });
        let pacer = self.pacer(&window, display_hz);
        // winit's redraw requests cannot ask for a lower rate, so only a display link steps.
        let fixed = matches!(pacer, Pacer::Redraw(_));
        let frames = FrameLoop::new(client, pacer)
            .with_feed(feed)
            .with_assets(assets)
            .paused_at(self.options.pause_at);
        let frames = if fixed {
            frames.fixed_at(display_hz, chosen.tier_can_drop())
        } else {
            frames.paced(display_hz, chosen.tier_can_drop())
        }
        .with_tier(chosen);
        #[cfg(feature = "drive")]
        let frames = frames.driven(self.drive.take());
        let mut frames = frames;
        // Whether `Cmd` in a shortcut is the Command key or Control.
        frames.handle(Input::Device(DeviceInput::Platform {
            mac: cfg!(target_os = "macos"),
        }));
        frames.handle(translator.surface());
        let timing = (!fixed).then_some(self.started);
        if let Some(epoch) = timing {
            frames.start_timing(epoch);
        }
        Ok(Running {
            frames,
            screen: Screen::new(
                renderer,
                surface,
                Arc::clone(&window),
                self.options.slow_gpu,
                timing,
            ),
            translator,
            clipboard: Clipboard::default(),
            fullscreen: is_fullscreen(&window),
            window,
        })
    }

    #[cfg_attr(not(target_os = "macos"), expect(clippy::unused_self))]
    fn pacer(&self, window: &Arc<Window>, display_hz: f32) -> Pacer {
        #[cfg(target_os = "macos")]
        if let Some(link) =
            crate::macos::DisplayLink::new(window, self.proxy.clone(), display_hz, self.started)
        {
            return Pacer::Link(link);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = display_hz;
        Pacer::Redraw(Arc::clone(window))
    }

    /// The display's highest refresh rate in frames a second, or 60 where it is unknown.
    #[expect(clippy::unused_self)]
    fn display_hz(&self, window: &Window) -> f32 {
        let millihertz = window.current_monitor().and_then(|monitor| {
            monitor
                .video_modes()
                .map(|mode| mode.refresh_rate_millihertz())
                .max()
                .or_else(|| monitor.refresh_rate_millihertz())
        });
        millihertz.unwrap_or(60_000) as f32 / 1000.0
    }

    /// Whether the run has lasted `Options::end_after`.
    fn over(&self) -> bool {
        self.options
            .end_after
            .is_some_and(|end| self.started.elapsed() >= end)
    }

    fn tick(&mut self, tick: Option<DisplayTick>) {
        let now = self.now();
        let Some(running) = &mut self.running else {
            return;
        };
        let tier = running.frames.client().tier();
        running.screen.begin(tick);
        let drawn = running.frames.tick_with_cadence(
            now,
            &mut running.screen,
            tick.and_then(|tick| tick.reported_hz),
        );
        running.screen.drain(&mut running.frames);
        let requests = tick.map(|_| Instant::now());
        if drawn.is_some() {
            while let Some(request) = running.frames.next_request() {
                if let Some(answer) = carry_out(&running.window, &mut running.clipboard, request) {
                    running.frames.handle(answer);
                }
            }
        }
        if let Some(tick) = tick {
            let mut cpu = running.frames.cpu_timing();
            let draw = running.screen.timing();
            cpu.draw_ns = draw.total_ns;
            cpu.drawable_wait_ns = draw.wait_ns;
            cpu.encode_submit_ns = draw.encode_ns;
            cpu.present_call_ns = draw.present_ns;
            cpu.requests_ns = requests.map_or(0, elapsed);
            let animating =
                draw.ambient_motion || running.frames.client().playing_verbs().next().is_some();
            running.frames.note_timing(&Timing::Frame(FrameTiming {
                tick,
                handled_ns: now,
                finished_ns: elapsed(self.started),
                outcome: draw.outcome.unwrap_or(Outcome::Idle),
                submitted_ns: draw.submitted_ns,
                cpu: drawn.map(|_| cpu),
                render_size: running.screen.size(),
                fullscreen: running.fullscreen,
                tier: tier.into(),
                animating,
                searching: false, // The app's feed adds its session status.
                gpu_in_flight: draw.gpu_in_flight,
                presented_ns: None,
            }));
        }
    }
}

impl<S: FnOnce(&mut Renderer, Opening) -> Chosen> ApplicationHandler<UserEvent> for App<S> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(running) = &mut self.running {
            running
                .frames
                .handle(Input::Device(DeviceInput::Lifecycle(Lifecycle::Resumed)));
            return;
        }
        match self.open(event_loop) {
            Ok(mut running) => {
                // Inputs sent before the window opened woke a loop with no client to take them.
                for input in self.inbox.try_iter() {
                    running.frames.handle(input);
                }
                self.running = Some(running);
            }
            Err(error) => {
                self.error = Some(error);
                event_loop.exit();
            }
        }
    }

    /// winit calls this as the event loop ends: after `exit`, and on macOS from
    /// `applicationWillTerminate:` on Cmd+Q or a quit from the Dock, after which the process
    /// ends without `run_app` returning.
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(running) = &mut self.running {
            running.frames.exiting();
        }
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(running) = &mut self.running {
            running
                .frames
                .handle(Input::Device(DeviceInput::Lifecycle(Lifecycle::Suspended)));
        }
    }

    fn memory_warning(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(running) = &mut self.running {
            running.frames.low_memory(&mut running.screen.renderer);
            running
                .frames
                .handle(Input::Device(DeviceInput::Lifecycle(Lifecycle::LowMemory)));
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            #[cfg(target_os = "macos")]
            UserEvent::Tick(tick) => self.tick(Some(tick)),
            UserEvent::Inbox => {
                if let Some(running) = &mut self.running {
                    for input in self.inbox.try_iter() {
                        running.frames.handle(input);
                    }
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if matches!(event, WindowEvent::RedrawRequested) {
            let redraw = self
                .running
                .as_mut()
                .is_some_and(|running| matches!(running.frames.ticks(), Pacer::Redraw(_)));
            if redraw {
                self.tick(None);
            }
            return;
        }
        let mode = self.options.mode;
        let Some(running) = &mut self.running else {
            return;
        };
        match &event {
            WindowEvent::Occluded(occluded) => running.frames.set_monitor_visible(!occluded),
            WindowEvent::CloseRequested => {
                event_loop.exit();
                return;
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key,
                        state,
                        repeat,
                        text,
                        ..
                    },
                ..
            } => {
                if mode == WindowMode::Background {
                    return;
                }
                let modifiers = running.translator.modifiers();
                if is_fullscreen_key(*physical_key, modifiers.control, modifiers.command) {
                    if *state == ElementState::Pressed && !repeat {
                        set_fullscreen(&running.window, !running.fullscreen);
                        running.fullscreen = is_fullscreen(&running.window);
                    }
                    return;
                }
                if let Some(input) = running.translator.key(*physical_key, *state, *repeat) {
                    running.frames.handle(input);
                }
                if let Some(input) = running.translator.typed(text.as_deref(), *state) {
                    running.frames.handle(input);
                }
                return;
            }
            _ => {}
        }
        if let Some(input) = running.translator.window_event(
            &event,
            running.frames.client().data().pointer.wheel_notch_points,
        ) {
            if matches!(input, Input::Device(DeviceInput::Surface { .. })) {
                let size = running.translator.size();
                running.screen.renderer.resize(
                    &mut running.screen.surface,
                    size.width,
                    size.height,
                );
            }
            running.frames.handle(input);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.over() {
            event_loop.exit();
            return;
        }
        let now = self.now();
        let end = self.options.end_after.map(|end| self.started + end);
        let Some(running) = &mut self.running else {
            return;
        };
        if let Some(driven) = running.frames.at_end() {
            self.error = driven.err();
            event_loop.exit();
            return;
        }
        #[cfg(target_os = "macos")]
        let was_running = match running.frames.ticks_ref() {
            Pacer::Link(link) => Some(link.state(now).running),
            Pacer::Redraw(_) => None,
        };
        let wait = match running.frames.schedule(now) {
            Sleep::UntilEvent => None,
            Sleep::Until(at) => Some(self.started + Duration::from_nanos(at)),
        };
        #[cfg(target_os = "macos")]
        if let Pacer::Link(link) = running.frames.ticks_ref() {
            let state = link.state(elapsed(self.started));
            if Some(state.running) != was_running {
                running.frames.note_timing(&Timing::Pacer(state));
            }
        }
        running.screen.drain(&mut running.frames);
        let flow = match (wait, end) {
            (Some(wait), Some(end)) => ControlFlow::WaitUntil(wait.min(end)),
            (Some(at), None) | (None, Some(at)) => ControlFlow::WaitUntil(at),
            (None, None) => ControlFlow::Wait,
        };
        event_loop.set_control_flow(flow);
    }
}

/// Makes `window` cover the whole display, or puts it back. On macOS this is winit's "simple"
/// fullscreen: the window grows over the display and hides the menu bar and the Dock, and the
/// window server composes it as it does any window. A fullscreen Space, which winit's
/// `Fullscreen::Borderless` makes on macOS, lets the system show the surface directly on a
/// `ProMotion` display, with a variable refresh: there the display link's own timestamps skipped
/// 12.5 ms or more about twice a second under load, so frames came late at every rate down to
/// 30 Hz, while the composed window held 120 Hz (docs/beryl-3d.md, "Measurements").
fn set_fullscreen(window: &Window, on: bool) {
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::WindowExtMacOS;
        window.set_simple_fullscreen(on);
    }
    #[cfg(not(target_os = "macos"))]
    window.set_fullscreen(on.then_some(winit::window::Fullscreen::Borderless(None)));
}

/// Whether `window` covers the whole display (`set_fullscreen`).
fn is_fullscreen(window: &Window) -> bool {
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::WindowExtMacOS;
        window.simple_fullscreen()
    }
    #[cfg(not(target_os = "macos"))]
    window.fullscreen().is_some()
}

/// Carries out one of the client's requests, and returns the input that answers it, if any. A
/// desktop has no haptics, and its text fields are edited in place, so text entry only turns
/// the input method on over the field.
fn carry_out(window: &Window, clipboard: &mut Clipboard, request: Request) -> Option<Input> {
    match request {
        Request::BeginTextEntry { rect, .. } => {
            window.set_ime_allowed(true);
            window.set_ime_cursor_area(
                LogicalPosition::new(rect.x, rect.y),
                LogicalSize::new(rect.width, rect.height),
            );
        }
        Request::EndTextEntry => window.set_ime_allowed(false),
        Request::Copy(text) => clipboard.copy(text),
        Request::Paste => {
            return clipboard
                .paste()
                .map(|text| Input::Device(DeviceInput::Paste(text)));
        }
        Request::Share { path } => reveal(&path),
        Request::OpenUrl(url) => open_url(url),
        // A desktop has no haptics. The app's feed saves the settings, plays the sound,
        // replays a recording, takes the bug report's picture, saves the report, exports the
        // reports and ends the run on a quit, as it sees each request first (`Feed::request`).
        Request::Haptic(_)
        | Request::ExportBugs
        | Request::FreezeFrame
        | Request::SaveBug { .. }
        | Request::SaveSettings(_)
        | Request::Music(_)
        | Request::Audio { .. }
        | Request::Replay { .. }
        | Request::Quit => {}
    }
    None
}

/// Shows the file at `path` in the system's file browser, on a thread of its own so that the
/// frame thread never waits for it.
fn reveal(path: &Path) {
    #[cfg(target_os = "macos")]
    let command = ("open", vec!["-R".into(), path.as_os_str().to_owned()]);
    #[cfg(not(target_os = "macos"))]
    let command = (
        "explorer",
        vec![{
            let mut argument = std::ffi::OsString::from("/select,");
            argument.push(path.as_os_str());
            argument
        }],
    );
    spawn(command.0, command.1);
}

/// Opens `url` in the default browser, on a thread of its own.
fn open_url(url: String) {
    #[cfg(target_os = "macos")]
    spawn("open", vec![url.into()]);
    #[cfg(not(target_os = "macos"))]
    spawn(
        "cmd",
        vec!["/C".into(), "start".into(), "".into(), url.into()],
    );
}

fn spawn(program: &'static str, arguments: Vec<std::ffi::OsString>) {
    std::thread::spawn(move || {
        if let Err(error) = Command::new(program).args(&arguments).status() {
            eprintln!("{program} failed: {error}");
        }
    });
}
