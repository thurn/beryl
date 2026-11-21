//! The windowless host (docs/beryl-3d.md, "Hosts" and "Driving the running app"): it draws the
//! client's frames with the real renderer into an offscreen target, on ticks from a timer at a
//! display's rate, so that a run with no window measures everything up to presentation. It is
//! what an agent's run of the app gets unless it names a window.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};

use app_client::Client;
use app_client::Input;
use beryl_assets::light;
use beryl_render::{Limits, Renderer, Target};
use kit_frame::frame::Frame;
use kit_ui::input::{Insets, Size};

use crate::assets::{Faces, Glyphs, Scans};
use crate::frames::{Chosen, Draw, Feed, FrameLoop, Opened, Opening, Shown, Sleep, Ticks, When};
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
    /// How long each finished frame is held back before it counts as shown, to imitate a
    /// slow GPU (`--slow-gpu`). Zero holds none back.
    pub slow_gpu: Duration,
}

/// The windowless host before it runs.
pub struct Windowless {
    options: WindowlessOptions,
    sender: Sender<Input>,
    receiver: Receiver<Input>,
    #[cfg(feature = "drive")]
    drive: Option<crate::drive::Drive>,
}

impl Windowless {
    #[must_use]
    pub fn new(options: WindowlessOptions) -> Self {
        let (sender, receiver) = channel();
        Self {
            options,
            sender,
            receiver,
            #[cfg(feature = "drive")]
            drive: None,
        }
    }

    /// The same host playing `drive` on the display's clock, ending the run after its last
    /// action.
    #[cfg(feature = "drive")]
    #[must_use]
    pub fn driven(mut self, drive: Option<crate::drive::Drive>) -> Self {
        self.drive = drive;
        self
    }

    /// A handle other threads use to send inputs. The host waits on the same channel, so
    /// sending wakes it with nothing more.
    pub fn inbox(&self) -> Inbox {
        Inbox::new(self.sender.clone(), || true)
    }

    /// Runs `client` with `feed` added to its frame loop until `end_after` has passed, the
    /// drive script has given its last action or the feed has ended, or for ever when none of
    /// these happens. `setup` prepares the renderer before the first frame, and chooses the tier
    /// the client starts in.
    ///
    /// # Errors
    ///
    /// When there is no GPU adapter or device, or a drive script's locator found nothing.
    pub fn run(
        self,
        client: Client,
        mut feed: Box<dyn Feed>,
        setup: impl FnOnce(&mut Renderer, Opening) -> Chosen,
    ) -> Result<(), String> {
        let options = self.options;
        let mut renderer = Renderer::headless(Limits::Default)?;
        renderer.upload_page(&light::prepared());
        let opening = opening(&options);
        let chosen = setup(&mut renderer, opening);
        let target = Target::offscreen(&renderer, opening.width, opening.height);
        feed.opened(&Opened {
            display_hz: options.hz,
            adapter: renderer.adapter().to_owned(),
        });
        crate::threads::interactive_thread();
        let timer = Timer {
            running: false,
            next: None,
            interval: interval_at(options.hz),
        };
        // The timer stands in for a display of `options.hz`, and steps down as a display link
        // does, so that `--slow-gpu` shows the rule at work in a run with no window.
        let frames = FrameLoop::new(client, timer)
            .with_feed(feed)
            .paused_at(options.pause_at)
            .paced(options.hz, chosen.tier_can_drop())
            .with_tier(chosen);
        #[cfg(feature = "drive")]
        let frames = frames.driven(self.drive);
        let mut frames = frames;
        frames.handle(Input::Surface {
            size: options.size,
            scale: options.scale,
            safe_area: Insets::default(),
        });
        // The host waits on its own channel, so a nudge down it wakes the loop.
        let wake = Inbox::new(self.sender.clone(), || true);
        let faces = Faces::start(&Scans::app(), beryl_art::SCRYFALL, {
            let wake = wake.clone();
            move || {
                wake.send(Input::Nudge);
            }
        });
        let mut screen = Offscreen {
            faces,
            renderer,
            target,
            slow_gpu: options.slow_gpu,
            glyphs: Glyphs::start(options.scale, move || {
                wake.send(Input::Nudge);
            }),
        };
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
            if let Some(driven) = frames.at_end() {
                return driven;
            }
            let since = |at: Instant| at.duration_since(started).as_nanos() as u64;
            let sleep = frames.schedule(since(now));
            let timer = frames.ticks();
            let interval = timer.interval;
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
                    // The feed sees each request (`Feed::request`); with no window, the host
                    // itself carries out none.
                    if frames.tick(since(now), &mut screen).is_some() {
                        while frames.next_request().is_some() {}
                    }
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
struct Timer {
    running: bool,
    /// When the next tick is due, while running.
    next: Option<Instant>,
    /// The time between ticks: one over the rate the frame loop asked for.
    interval: Duration,
}

impl Ticks for Timer {
    fn set_running(&mut self, running: bool) {
        self.running = running;
        if !running {
            self.next = None;
        }
    }

    fn set_rate(&mut self, hz: f32) {
        self.interval = interval_at(hz);
    }
}

/// The time between ticks at `hz` frames a second.
fn interval_at(hz: f32) -> Duration {
    Duration::from_secs_f64(1.0 / f64::from(hz.max(1.0)))
}

/// The offscreen target frames are drawn into.
struct Offscreen {
    renderer: Renderer,
    target: Target,
    slow_gpu: Duration,
    glyphs: Glyphs,
    faces: Faces,
}

impl Draw for Offscreen {
    fn draw(&mut self, frame: &Frame) -> Option<Shown> {
        let started = Instant::now();
        self.renderer.render(frame, &mut self.target);
        if !self.slow_gpu.is_zero() {
            std::thread::sleep(self.slow_gpu);
        }
        Some(Shown {
            waited: 0,
            encoded: started.elapsed().as_nanos() as u64,
        })
    }

    fn capture(&mut self, _frame: &Frame) -> Option<beryl_render::Readback> {
        Some(self.renderer.read_back(&self.target))
    }

    fn assets(&mut self, client: &mut Client, when: When) {
        self.glyphs.deliver(client, &mut self.renderer);
        self.faces.deliver(client, &mut self.renderer, when);
    }
}

/// The offscreen target's size in physical pixels, its scale and the timer's rate.
fn opening(options: &WindowlessOptions) -> Opening {
    Opening {
        width: (options.size.width * options.scale).round().max(1.0) as u32,
        height: (options.size.height * options.scale).round().max(1.0) as u32,
        scale: options.scale,
        display_hz: options.hz,
    }
}
