//! The windowless host (docs/beryl-3d.md, "Hosts" and "Driving the running app"): it draws the
//! client's frames with the real renderer into an offscreen target, on ticks from a timer at a
//! display's rate, so that a run with no window measures everything up to presentation. It is
//! what an agent's run of the app gets unless it names a window.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};

use beryl_render::{Limits, Renderer, Target};
use beryl_scene::Client;
use beryl_scene::frame::Frame;
use beryl_scene::input::{Input, Insets, Size};

use crate::frames::{Draw, Feed, FrameLoop, Opened, Shown, Sleep, Ticks};
use crate::inbox::Inbox;

/// How the windowless host draws.
#[derive(Clone, Debug)]
pub struct WindowlessOptions {
    /// The size of the window it stands in for, in logical points.
    pub size: Size,
    /// Physical pixels per logical point.
    pub scale: f32,
    /// The rate of its ticks, in frames a second.
    pub hz: f32,
    /// Where the client's clock stops, in nanoseconds since it started.
    pub pause_at: Option<u64>,
    /// How long it runs before it ends by itself, if it does.
    pub end_after: Option<Duration>,
}

/// The windowless host before it runs.
pub struct Windowless {
    options: WindowlessOptions,
    sender: Sender<Input>,
    receiver: Receiver<Input>,
}

impl Windowless {
    #[must_use]
    pub fn new(options: WindowlessOptions) -> Self {
        let (sender, receiver) = channel();
        Self {
            options,
            sender,
            receiver,
        }
    }

    /// A handle other threads use to send inputs. The host waits on the same channel, so
    /// sending wakes it with nothing more.
    pub fn inbox(&self) -> Inbox {
        Inbox::new(self.sender.clone(), || true)
    }

    /// Runs `client` with `feed` added to its frame loop until `end_after` has passed, or for
    /// ever when it is not given. `setup` prepares the renderer before the first frame.
    ///
    /// # Errors
    ///
    /// When there is no GPU adapter or device.
    pub fn run(
        self,
        client: Client,
        mut feed: Box<dyn Feed>,
        setup: impl FnOnce(&mut Renderer),
    ) -> Result<(), String> {
        let options = self.options;
        let mut renderer = Renderer::headless(Limits::Default)?;
        setup(&mut renderer);
        let width = (options.size.width * options.scale).round().max(1.0) as u32;
        let height = (options.size.height * options.scale).round().max(1.0) as u32;
        let target = Target::offscreen(&renderer, width, height);
        feed.opened(&Opened {
            display_hz: options.hz,
            adapter: renderer.adapter().to_owned(),
        });
        crate::threads::interactive_thread();
        let mut frames = FrameLoop::new(client, Timer::default())
            .with_feed(feed)
            .paused_at(options.pause_at);
        frames.handle(Input::Surface {
            size: options.size,
            scale: options.scale,
            safe_area: Insets::default(),
        });
        let mut screen = Offscreen { renderer, target };
        let interval = Duration::from_secs_f64(1.0 / f64::from(options.hz.max(1.0)));
        let started = Instant::now();
        let end = options.end_after.map(|end| started + end);
        loop {
            for input in self.receiver.try_iter() {
                frames.handle(input);
            }
            let now = Instant::now();
            if end.is_some_and(|end| now >= end) {
                return Ok(());
            }
            let since = |at: Instant| at.duration_since(started).as_nanos() as u64;
            let sleep = frames.schedule(since(now));
            let timer = frames.ticks();
            let wake = if timer.running {
                let tick = *timer.next.get_or_insert(now);
                if tick <= now {
                    // A tick missed while the last frame was drawn is skipped, as a display
                    // skips a refresh nobody was ready for.
                    let mut next = tick + interval;
                    while next <= now {
                        next += interval;
                    }
                    timer.next = Some(next);
                    frames.tick(since(now), &mut screen);
                    continue;
                }
                Some(tick)
            } else {
                match sleep {
                    Sleep::Until(at) => Some(started + Duration::from_nanos(at)),
                    Sleep::UntilEvent => None,
                }
            };
            let until = match (wake, end) {
                (Some(wake), Some(end)) => Some(wake.min(end)),
                (Some(at), None) | (None, Some(at)) => Some(at),
                (None, None) => None,
            };
            let received = match until {
                Some(until) => self
                    .receiver
                    .recv_timeout(until.saturating_duration_since(Instant::now())),
                None => self
                    .receiver
                    .recv()
                    .map_err(|_| RecvTimeoutError::Disconnected),
            };
            // The host holds a sender of its own, so the channel is never disconnected and an
            // error is always a timeout.
            if let Ok(input) = received {
                frames.handle(input);
            }
        }
    }
}

/// Ticks from a timer: while running, one every interval from when they started.
#[derive(Default)]
struct Timer {
    running: bool,
    /// When the next tick is due, while running.
    next: Option<Instant>,
}

impl Ticks for Timer {
    fn set_running(&mut self, running: bool) {
        self.running = running;
        if !running {
            self.next = None;
        }
    }
}

/// The offscreen target frames are drawn into.
struct Offscreen {
    renderer: Renderer,
    target: Target,
}

impl Draw for Offscreen {
    fn draw(&mut self, frame: &Frame) -> Option<Shown> {
        let started = Instant::now();
        self.renderer.render(frame, &mut self.target);
        Some(Shown {
            waited: 0,
            encoded: started.elapsed().as_nanos() as u64,
        })
    }
}
