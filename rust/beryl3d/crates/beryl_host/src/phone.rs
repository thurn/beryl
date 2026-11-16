//! The part of the iOS and Android hosts that is Rust (docs/beryl-3d.md, "Hosts"). The
//! platform's own app owns the view and the display's ticks, and calls in: it passes the
//! native surface when it has one, says when it loses it, hands over pointer, key and text
//! events, asks for a frame on each tick of the display, and carries out what the client asks
//! of the platform. Everything else (the frame loop, the renderer, inputs from other threads)
//! happens here, so the Swift and Kotlin apps hold no logic of their own.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

use beryl_assets::light;
#[cfg(feature = "headless")]
use beryl_render::Target;
use beryl_render::{Native, Presented, Renderer, Surface};
use beryl_scene::Client;
use beryl_scene::command::Request;
use beryl_scene::frame::{Frame, Wake};
use beryl_scene::input::{Input, Insets, Key, Lifecycle, Pointer, Size, TextChange};

use crate::frames::{Draw, Feed, FrameLoop, Opened, Shown, Ticks};
use crate::inbox::Inbox;

/// What the platform does when an input arrives from another thread: it asks for a frame
/// soon, hopping to its main thread itself. Called from any thread.
pub type Wakeup = Arc<dyn Fn() + Send + Sync>;

/// A phone host: the client and its feed, and once the platform has passed a surface, the
/// renderer and the frame loop.
pub struct Phone {
    state: State,
    receiver: Receiver<Input>,
    /// The display's highest refresh rate, as the platform reads it.
    display_hz: f32,
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

impl Phone {
    /// A phone host for `client`, on a display whose highest rate is `display_hz`. `feed` is
    /// made with the inbox other threads (the session's) send inputs to; each one sent also
    /// calls `wake`, if the platform gave one, since it may have paused its ticks.
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
        Self {
            state: State::Waiting(Box::new((client, feed(inbox)))),
            receiver,
            display_hz,
            started: None,
        }
    }

    /// The rate the platform should ask its display for from now on.
    #[must_use]
    pub fn hz(&self) -> f32 {
        60.0
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
    /// [`Phone::surface_lost`] is called or the phone host is dropped.
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
                // SAFETY: the caller's promise, passed on.
                match unsafe { Renderer::for_native(native, width, height) } {
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
                let renderer = match Renderer::headless(beryl_render::Limits::Downlevel) {
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
        mut renderer: Renderer,
        output: Output,
        size: Input,
    ) {
        renderer.upload_page(&light::prepared());
        feed.opened(&Opened {
            display_hz: self.display_hz,
            adapter: renderer.adapter().to_owned(),
        });
        crate::threads::interactive_thread();
        // The phone hosts cannot change their display's rate yet, so frames are counted
        // against 60 Hz and the rate never steps.
        let mut frames = FrameLoop::new(client, Platform)
            .with_feed(feed)
            .fixed_at(60.0);
        frames.handle(size);
        self.state = State::Running(Box::new(Running {
            frames,
            screen: Screen { renderer, output },
        }));
    }

    /// The platform took the surface away (Android destroys it in the background) or forbids
    /// drawing in it (iOS in the background). The renderer drops it, and draws nothing until
    /// the next [`Phone::surface`].
    pub fn surface_lost(&mut self) {
        if let State::Running(running) = &mut self.state
            && let Output::Surface(surface) = &mut running.screen.output
        {
            *surface = None;
        }
    }

    /// The app was suspended or resumed, or memory is low.
    pub fn lifecycle(&mut self, lifecycle: Lifecycle) {
        self.give(Input::Lifecycle(lifecycle));
    }

    /// A mouse, touch or pen event. Dropped before the first surface.
    pub fn pointer(&mut self, pointer: Pointer) {
        self.give(Input::Pointer(pointer));
    }

    /// A key event. Dropped before the first surface.
    pub fn key(&mut self, key: Key) {
        self.give(Input::Key(key));
    }

    /// The edited text field's whole new contents. Dropped before the first surface.
    pub fn text(&mut self, text: String) {
        self.give(Input::Text(TextChange(text)));
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
        frames.tick(since, screen);
        frames.schedule(since);
        match frames.next_wake(since) {
            Wake::At(at) => Wake::At(started + at),
            wake => wake,
        }
    }
}

/// The `Surface` input for a surface of `width` by `height` physical pixels at `scale`.
fn surface_input(width: u32, height: u32, scale: f32, safe_area: Insets) -> Input {
    Input::Surface {
        size: Size {
            width: width as f32 / scale,
            height: height as f32 / scale,
        },
        scale,
        safe_area,
    }
}

/// The frame loop and what it draws on, once the platform has passed a surface.
struct Running {
    frames: FrameLoop<Platform>,
    screen: Screen,
}

/// The platform's ticks, which the Swift or Kotlin app runs for as long as it is in front.
struct Platform;

impl Ticks for Platform {
    fn set_running(&mut self, _running: bool) {}
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
                    Presented::Shown { waited, .. } => {
                        let waited = waited.as_nanos() as u64;
                        let total = started.elapsed().as_nanos() as u64;
                        Some(Shown {
                            waited,
                            encoded: total.saturating_sub(waited),
                        })
                    }
                    Presented::Skipped => None,
                }
            }
            #[cfg(feature = "headless")]
            Output::Target(target) => {
                self.renderer.render(frame, target);
                Some(Shown {
                    waited: 0,
                    encoded: started.elapsed().as_nanos() as u64,
                })
            }
        }
    }
}
