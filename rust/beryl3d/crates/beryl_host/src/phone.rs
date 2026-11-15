//! The part of the iOS and Android hosts that is Rust (docs/beryl-3d.md, "Hosts"). The
//! platform's own app owns the view and the display's ticks, and calls in: it passes the
//! native surface when it has one, says when it loses it, and asks for a frame on each tick of
//! the display. Everything else (the frame loop, the renderer, inputs from other threads)
//! happens here, so the Swift and Kotlin apps hold no logic of their own.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

use beryl_render::{Native, Presented, Renderer, Surface};
use beryl_scene::Client;
use beryl_scene::frame::{Frame, Wake};
use beryl_scene::input::{Input, Insets, Lifecycle, Size};

use crate::frames::{Draw, Feed, FrameLoop, Opened, Shown, Ticks};
use crate::inbox::Inbox;

/// A phone host: the client and its feed, and once the platform has passed a surface, the
/// renderer and the frame loop.
pub struct Phone {
    state: State,
    sender: Sender<Input>,
    receiver: Receiver<Input>,
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
    #[must_use]
    pub fn new(client: Client, feed: Box<dyn Feed>) -> Self {
        let (sender, receiver) = channel();
        Self {
            state: State::Waiting(Box::new((client, feed))),
            sender,
            receiver,
            started: None,
        }
    }

    /// A handle other threads use to send inputs. They are given to the client at the next
    /// frame the platform asks for; the platform's ticks never stop, so nothing more wakes it.
    pub fn inbox(&self) -> Inbox {
        Inbox::new(self.sender.clone(), || true)
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
        let size = Input::Surface {
            size: Size {
                width: width as f32 / scale,
                height: height as f32 / scale,
            },
            scale,
            safe_area,
        };
        match std::mem::replace(&mut self.state, State::Moving) {
            State::Waiting(waiting) => {
                let (client, mut feed) = *waiting;
                // SAFETY: the caller's promise, passed on.
                let made = unsafe { Renderer::for_native(native, width, height) };
                let (renderer, surface) = match made {
                    Ok(made) => made,
                    Err(error) => {
                        self.state = State::Waiting(Box::new((client, feed)));
                        return Err(error);
                    }
                };
                feed.opened(&Opened {
                    // The hosts ask for the display's highest rate; the spike's hosts do not
                    // read it back, so the floor's 60 Hz is assumed.
                    display_hz: 60.0,
                    adapter: renderer.adapter().to_owned(),
                });
                crate::threads::interactive_thread();
                let mut frames = FrameLoop::new(client, Platform).with_feed(feed);
                frames.handle(size);
                self.state = State::Running(Box::new(Running {
                    frames,
                    screen: Screen {
                        renderer,
                        surface: Some(surface),
                    },
                }));
                Ok(())
            }
            State::Running(mut running) => {
                let Running { frames, screen } = &mut *running;
                let attached = match &mut screen.surface {
                    Some(surface) => {
                        screen.renderer.resize(surface, width, height);
                        Ok(())
                    }
                    // SAFETY: the caller's promise, passed on.
                    None => unsafe { screen.renderer.attach(native, width, height) }
                        .map(|surface| screen.surface = Some(surface)),
                };
                frames.handle(size);
                self.state = State::Running(running);
                attached
            }
            State::Moving => unreachable!("a phone host is never left moving"),
        }
    }

    /// The platform took the surface away (Android destroys it in the background) or forbids
    /// drawing in it (iOS in the background). The renderer drops it, and draws nothing until
    /// the next [`Phone::surface`].
    pub fn surface_lost(&mut self) {
        if let State::Running(running) = &mut self.state {
            running.screen.surface = None;
        }
    }

    /// The app was suspended or resumed, or memory is low.
    pub fn lifecycle(&mut self, lifecycle: Lifecycle) {
        if let State::Running(running) = &mut self.state {
            running.frames.handle(Input::Lifecycle(lifecycle));
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
        match frames.client().wake() {
            Wake::At(at) => Wake::At(started + at),
            wake => wake,
        }
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

/// The renderer and the surface it draws in, while the platform has given one.
struct Screen {
    renderer: Renderer,
    surface: Option<Surface>,
}

impl Draw for Screen {
    fn draw(&mut self, frame: &Frame) -> Option<Shown> {
        let surface = self.surface.as_mut()?;
        let started = Instant::now();
        match self.renderer.present(frame, surface, || {}) {
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
}
