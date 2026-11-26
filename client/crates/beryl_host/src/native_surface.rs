//! The part of the iOS and Android hosts that is Rust (docs/beryl-3d.md, "Hosts"). The
//! platform's own app owns the view and the display's ticks, and calls in: it passes the
//! native surface when it has one, says when it loses it, hands over pointer, key and text
//! events, asks for a frame on each tick of the display, and carries out what the client asks
//! of the platform. Everything else (the frame loop, the renderer, inputs from other threads)
//! happens here, so the Swift and Kotlin apps hold no logic of their own.

use kit_ui::input::Input as DeviceInput;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

use app_client::Client;
use app_client::Input;
use app_client::command::Request;
#[cfg(feature = "headless")]
use beryl_render::Target;
use beryl_render::{Native, Presented, Renderer, Surface};
use kit_frame::frame::{Frame, Wake};
use kit_ui::input::{Insets, Key, Lifecycle, Pointer, Size, TextChange};

use crate::assets::Assets;
use crate::frames::{Chosen, Draw, Feed, FrameLoop, Opened, Shown, Ticks};
use crate::inbox::Inbox;

/// What the platform does when an input arrives from another thread: it asks for a frame
/// soon, hopping to its main thread itself. Called from any thread.
pub type Wakeup = Arc<dyn Fn() + Send + Sync>;

/// The client and its feed for iOS and Android. When the platform supplies a surface, the
/// host starts its renderer and frame loop.
pub struct NativeSurface {
    state: State,
    receiver: Receiver<Input>,
    /// Where the glyph and face workers wake the host when they finish something.
    inbox: Inbox,
    /// The display's highest refresh rate, as the platform reads it.
    display_hz: f32,
    /// The platform's time of the last tick that drew a frame, in nanoseconds since
    /// `started`.
    last_drawn: Option<u64>,
    /// The platform's time of the first frame asked for, in nanoseconds: the client's clock
    /// counts from it.
    started: Option<u64>,
}

enum State {
    /// No surface has been passed yet, so there is no renderer.
    Waiting(Box<(Client, Box<dyn Feed>)>),
    Running(Box<Running>),
    /// Only while changing from one state to the other.
    Moving,
}

impl NativeSurface {
    /// Hosts `client` on a display whose highest rate is `display_hz`. The `feed` receives the
    /// inbox that other threads send inputs to. Sending an input also calls `wake` when the
    /// platform supplied a callback, so the platform can resume its ticks.
    #[must_use]
    pub fn new(
        client: Client,
        display_hz: f32,
        wake: Option<Wakeup>,
        feed: impl FnOnce(Inbox) -> Box<dyn Feed>,
    ) -> Self {
        let (sender, receiver): (Sender<Input>, _) = channel();
        let inbox = match wake {
            Some(wake) => Inbox::new(sender, move || {
                wake();
                true
            }),
            None => Inbox::new(sender, || true),
        };
        // A rate the platform could not read would leave the rule nothing to divide.
        let display_hz = if display_hz.is_finite() && display_hz >= 1.0 {
            display_hz
        } else {
            60.0
        };
        Self {
            state: State::Waiting(Box::new((client, feed(inbox.clone())))),
            receiver,
            inbox,
            display_hz,
            last_drawn: None,
            started: None,
        }
    }

    /// The rate the platform should ask its display for from now on: the rate rule's choice
    /// (`crate::rate`), which starts at the display's highest.
    #[must_use]
    pub fn hz(&self) -> f32 {
        match &self.state {
            State::Running(running) => running.frames.ticks_ref().hz(),
            _ => self.display_hz,
        }
    }

    /// The platform has a surface to draw in, `width` by `height` physical pixels at `scale`
    /// physical pixels per point. The first one makes the renderer on a GPU that can present
    /// to it; a later one, after the last was lost, gets a surface on the same GPU.
    ///
    /// # Errors
    ///
    /// When there is no GPU adapter or device for the surface.
    ///
    /// # Safety
    ///
    /// `native` must point to a live layer or window of its kind, and stay valid until
    /// [`NativeSurface::surface_lost`] is called or the host is dropped.
    #[allow(unsafe_code)]
    pub unsafe fn surface(
        &mut self,
        native: Native,
        width: u32,
        height: u32,
        scale: f32,
        safe_area: Insets,
    ) -> Result<(), String> {
        let size = surface_input(width, height, scale, safe_area);
        match std::mem::replace(&mut self.state, State::Moving) {
            State::Waiting(waiting) => {
                let lighting = waiting.0.data().lighting;
                // SAFETY: the caller's promise, passed on.
                match unsafe { Renderer::for_native(native, width, height, lighting) } {
                    Ok((renderer, surface)) => {
                        self.open(*waiting, renderer, Output::Surface(Some(surface)), size);
                        Ok(())
                    }
                    Err(error) => {
                        self.state = State::Waiting(waiting);
                        Err(error)
                    }
                }
            }
            State::Running(mut running) => {
                let Running { frames, screen } = &mut *running;
                let attached = match &mut screen.output {
                    Output::Surface(Some(surface)) => {
                        screen.renderer.resize(surface, width, height);
                        Ok(())
                    }
                    // SAFETY: the caller's promise, passed on.
                    Output::Surface(None) => {
                        unsafe { screen.renderer.attach(native, width, height) }
                            .map(|surface| screen.output = Output::Surface(Some(surface)))
                    }
                    // A headless renderer has no GPU instance to make a surface with.
                    #[cfg(feature = "headless")]
                    Output::Target(_) => {
                        Err("this phone host draws offscreen and takes no surface".to_owned())
                    }
                };
                if attached.is_ok() {
                    frames.set_monitor_visible(true);
                }
                frames.handle(size);
                self.state = State::Running(running);
                attached
            }
            State::Moving => unreachable!("a phone host is never left moving"),
        }
    }

    /// Draws into an offscreen target of `width` by `height` pixels at one pixel per point, in
    /// place of a native surface: for a test of the C functions, which has no view to give.
    ///
    /// # Errors
    ///
    /// When there is no GPU adapter or device.
    #[cfg(feature = "headless")]
    pub fn offscreen(&mut self, width: u32, height: u32) -> Result<(), String> {
        let size = surface_input(width, height, 1.0, Insets::default());
        match std::mem::replace(&mut self.state, State::Moving) {
            State::Waiting(waiting) => {
                let lighting = waiting.0.data().lighting;
                let renderer = match Renderer::headless(beryl_render::Limits::Downlevel, lighting) {
                    Ok(renderer) => renderer,
                    Err(error) => {
                        self.state = State::Waiting(waiting);
                        return Err(error);
                    }
                };
                let target = Target::offscreen(&renderer, width, height);
                self.open(*waiting, renderer, Output::Target(target), size);
            }
            State::Running(mut running) => {
                let screen = &mut running.screen;
                screen.output = Output::Target(Target::offscreen(&screen.renderer, width, height));
                running.frames.handle(size);
                self.state = State::Running(running);
            }
            State::Moving => unreachable!("a phone host is never left moving"),
        }
        Ok(())
    }

    /// Starts the frame loop on the first surface or target.
    fn open(
        &mut self,
        (client, mut feed): (Client, Box<dyn Feed>),
        renderer: Renderer,
        output: Output,
        size: Input,
    ) {
        feed.opened(&Opened {
            display_hz: self.display_hz,
            adapter: renderer.adapter().to_owned(),
        });
        crate::threads::interactive_thread();
        // The rule starts at the display's highest rate and steps as frames miss or keep it.
        // A phone's app measures nothing at startup, so the tier is Baseline and cannot drop.
        let chosen = Chosen::BASELINE;
        let mut frames = FrameLoop::new(client, Platform::default())
            .with_feed(feed)
            .with_assets(Assets::live({
                let inbox = self.inbox.clone();
                move || {
                    inbox.send(Input::Nudge);
                }
            }))
            .paced(self.display_hz, chosen.tier_can_drop())
            .with_tier(chosen);
        frames.handle(size);
        self.state = State::Running(Box::new(Running {
            frames,
            screen: Screen { renderer, output },
        }));
    }

    /// The platform took the surface away (Android destroys it in the background) or forbids
    /// drawing in it (iOS in the background). The renderer drops it, and draws nothing until
    /// the next [`NativeSurface::surface`].
    pub fn surface_lost(&mut self) {
        if let State::Running(running) = &mut self.state
            && let Output::Surface(surface) = &mut running.screen.output
        {
            *surface = None;
            running.frames.set_monitor_visible(false);
        }
    }

    /// The app was suspended or resumed, or memory is low. On low memory the renderer frees
    /// its full-size faces at once, beside the client forgetting them at its next input
    /// (docs/beryl-3d.md, "Hosts": "Low memory"). Before the platform has passed a surface
    /// there is no renderer and nothing to free, so the input is ignored.
    pub fn lifecycle(&mut self, lifecycle: Lifecycle) {
        if lifecycle == Lifecycle::LowMemory
            && let State::Running(running) = &mut self.state
        {
            running.frames.low_memory(&mut running.screen.renderer);
        }
        self.give(Input::Device(DeviceInput::Lifecycle(lifecycle)));
    }

    /// A mouse, touch or pen event. Dropped before the first surface.
    pub fn pointer(&mut self, pointer: Pointer) {
        self.give(Input::Device(DeviceInput::Pointer(pointer)));
    }

    /// A key event. Dropped before the first surface.
    pub fn key(&mut self, key: Key) {
        self.give(Input::Device(DeviceInput::Key(key)));
    }

    /// The edited text field's whole new contents. Dropped before the first surface.
    pub fn text(&mut self, text: String) {
        self.give(Input::Device(DeviceInput::Text(TextChange::Replace(text))));
    }

    /// The next thing the client asks of the platform, or `None`, as always before the first
    /// surface.
    pub fn next_request(&mut self) -> Option<Request> {
        match &mut self.state {
            State::Running(running) => running.frames.client().next_request(),
            _ => None,
        }
    }

    /// Hands `input` to the frame loop, once there is one.
    fn give(&mut self, input: Input) {
        if let State::Running(running) = &mut self.state {
            running.frames.handle(input);
        }
    }

    /// The display is ready for a frame at `now`, the platform's monotonic time in
    /// nanoseconds. Gives the client the inputs other threads sent, draws a frame if one is
    /// wanted, and says when the next is.
    pub fn frame(&mut self, now: u64) -> Wake {
        let State::Running(running) = &mut self.state else {
            return Wake::Never;
        };
        let Running { frames, screen } = &mut **running;
        for input in self.receiver.try_iter() {
            frames.handle(input);
        }
        let started = *self.started.get_or_insert(now);
        let since = now.saturating_sub(started);
        // A display that ticks faster than the rule's rate, because the platform cannot
        // change it or ignored the request, draws only on the ticks the rate falls on.
        if due(
            self.last_drawn,
            since,
            frames.ticks_ref().hz(),
            self.display_hz,
        ) {
            if frames.tick(since, screen).is_some() {
                self.last_drawn = Some(since);
            }
            frames.schedule(since);
        }
        match frames.next_wake(since) {
            Wake::At(at) => Wake::At(started + at),
            wake => wake,
        }
    }
}

/// Whether a tick of the display at `now` should draw, when the last tick that drew was at
/// `last_drawn` (both in nanoseconds), the rate rule has chosen `hz` and the display ticks at
/// up to `display_hz`. A tick draws once the chosen rate's interval has passed, less half of
/// the display's interval for the ticks' jitter: with the display at 120 Hz and the rule at 60
/// Hz, every other tick draws. The first tick always draws.
#[must_use]
pub fn due(last_drawn: Option<u64>, now: u64, hz: f32, display_hz: f32) -> bool {
    let Some(last) = last_drawn else {
        return true;
    };
    let interval = 1e9 / f64::from(hz.max(1.0));
    let display_interval = 1e9 / f64::from(display_hz.max(1.0));
    now.saturating_sub(last) as f64 >= interval - display_interval / 2.0
}

/// The `Surface` input for a surface of `width` by `height` physical pixels at `scale`.
fn surface_input(width: u32, height: u32, scale: f32, safe_area: Insets) -> Input {
    Input::Device(DeviceInput::Surface {
        size: Size {
            width: width as f32 / scale,
            height: height as f32 / scale,
        },
        scale,
        safe_area,
    })
}

/// The frame loop and what it draws on, once the platform has passed a surface.
struct Running {
    frames: FrameLoop<Platform>,
    screen: Screen,
}

/// The platform's ticks: the Swift or Kotlin app's display link or frame callback. The app
/// asks its display for the rate stored here, which it reads from each frame's wake, and pauses
/// its ticks when the wake says no frame is wanted.
#[derive(Debug)]
pub struct Platform {
    hz: f32,
}

impl Platform {
    /// The rate the frame loop last asked for.
    #[must_use]
    pub fn hz(&self) -> f32 {
        self.hz
    }
}

impl Default for Platform {
    fn default() -> Self {
        Self { hz: 60.0 }
    }
}

impl Ticks for Platform {
    fn set_running(&mut self, _running: bool) {}

    fn set_rate(&mut self, hz: f32) {
        self.hz = hz;
    }
}

/// The renderer and what it draws in.
struct Screen {
    renderer: Renderer,
    output: Output,
}

/// What frames are drawn in.
enum Output {
    /// The platform's surface, while it has given one.
    Surface(Option<Surface>),
    /// An offscreen target, for a test.
    #[cfg(feature = "headless")]
    Target(Target),
}

impl Draw for Screen {
    fn draw(&mut self, frame: &Frame) -> Option<Shown> {
        let started = Instant::now();
        match &mut self.output {
            Output::Surface(surface) => {
                match self.renderer.present(frame, surface.as_mut()?, || {}) {
                    Presented::Shown {
                        waited,
                        present_cpu,
                        ..
                    } => {
                        let waited = waited.as_nanos() as u64;
                        let presented = present_cpu.as_nanos() as u64;
                        let total = started.elapsed().as_nanos() as u64;
                        Some(Shown {
                            waited,
                            encoded: total.saturating_sub(waited).saturating_sub(presented),
                            presented,
                        })
                    }
                    Presented::Skipped { .. } => None,
                }
            }
            #[cfg(feature = "headless")]
            Output::Target(target) => {
                self.renderer.render(frame, target);
                Some(Shown {
                    waited: 0,
                    encoded: started.elapsed().as_nanos() as u64,
                    presented: 0,
                })
            }
        }
    }

    fn renderer(&mut self) -> Option<&mut Renderer> {
        Some(&mut self.renderer)
    }
}
