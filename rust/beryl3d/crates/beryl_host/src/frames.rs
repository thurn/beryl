//! The frame loop every host runs, apart from any window (docs/beryl-3d.md, "Hosts" and
//! "Drawing only when something changes"). An input reaches the client at once but is drawn
//! only on the next tick of the display, and ticks are asked for only while the client wants a
//! frame. Where ticks come from is a type parameter, so a test can supply them.

use std::time::Instant;

use beryl_scene::Client;
use beryl_scene::frame::{Frame, Wake};
use beryl_scene::input::Input;

/// A source of display ticks: a display link on macOS, winit's redraw requests elsewhere, a
/// timer in the windowless host, or a test's own calls.
pub trait Ticks {
    /// Starts ticks while `running` is true and stops them otherwise. Called after every batch
    /// of events, so a source may ask again on each call while running.
    fn set_running(&mut self, running: bool);
}

/// What drawing one frame cost, in nanoseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shown {
    /// Waiting for a drawable from the display: the window server's time, not the app's.
    pub waited: u64,
    /// Encoding the frame's GPU commands and submitting them, after the drawable came.
    pub encoded: u64,
}

/// Where a frame goes once the client has made it.
pub trait Draw {
    /// Draws `frame`, and says what it cost, or `None` when no drawable was available and
    /// nothing was shown.
    fn draw(&mut self, frame: &Frame) -> Option<Shown>;
}

/// One frame the loop drew.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ticked {
    /// The tick's time, in nanoseconds since the client started.
    pub tick: u64,
    /// The client's clock for the frame: the tick's time, or the pause's while paused.
    pub clock: u64,
    /// How long since the last frame, when the ticks ran without a stop in between, so that
    /// the gap measures frame delivery and not a still table.
    pub since_last: Option<u64>,
    /// How long `Client::frame` took.
    pub client: u64,
    /// What drawing cost, or `None` when nothing was shown.
    pub shown: Option<Shown>,
}

/// What the host found when it opened its window or target, for the app's log.
#[derive(Clone, Debug, PartialEq)]
pub struct Opened {
    /// The display's highest refresh rate, in frames a second: the rate of the ticks.
    pub display_hz: f32,
    /// The GPU adapter's name.
    pub adapter: String,
}

/// What a host's app adds to the loop: inputs it gives the client before a frame (a recording's
/// views), what it does with the client's queues after one (commands for the session thread),
/// and when it next needs a tick of its own.
pub trait Feed {
    /// Called once, when the host has opened its window or target and before the first frame.
    fn opened(&mut self, _opened: &Opened) {}

    /// Sees every input before the client does.
    fn input(&mut self, _input: &Input) {}

    /// Called on a tick before the frame is made, with the client's clock.
    fn before(&mut self, _client: &mut Client, _clock: u64) {}

    /// Called after each frame the loop drew.
    fn after(&mut self, _client: &mut Client, _ticked: &Ticked) {}

    /// When the feed next needs a frame, beside the client's own wake.
    fn wake(&self) -> Wake {
        Wake::Never
    }
}

/// A feed that adds nothing.
pub struct NoFeed;

impl Feed for NoFeed {}

/// How long the event loop may sleep before it must check the client again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sleep {
    /// Until the next event or tick.
    UntilEvent,
    /// Until this time, in nanoseconds since the client started, when the client wants a frame
    /// unless an event comes first.
    Until(u64),
}

/// The client, its source of ticks and the app's feed.
pub struct FrameLoop<T> {
    client: Client,
    ticks: T,
    feed: Box<dyn Feed>,
    /// Where the client's clock stops, in nanoseconds since it started (`--pause-at`).
    pause: Option<u64>,
    /// Whether a frame has been drawn at the pause, after which only an input draws another.
    drawn_at_pause: bool,
    /// Whether an input has arrived since the last frame.
    input_since: bool,
    /// When the last frame was drawn, while the ticks have run since.
    last: Option<u64>,
}

impl<T: Ticks> FrameLoop<T> {
    pub fn new(client: Client, ticks: T) -> Self {
        Self {
            client,
            ticks,
            feed: Box::new(NoFeed),
            pause: None,
            drawn_at_pause: false,
            input_since: false,
            last: None,
        }
    }

    /// The same loop with `feed` added.
    #[must_use]
    pub fn with_feed(mut self, feed: Box<dyn Feed>) -> Self {
        self.feed = feed;
        self
    }

    /// Stops the client's clock at `at`, in nanoseconds since it started. A frame is drawn at
    /// the pause, and after it only an input draws another, at the same time.
    #[must_use]
    pub fn paused_at(mut self, at: Option<u64>) -> Self {
        self.pause = at;
        self
    }

    /// Hands `input` to the client. Nothing is drawn until the next tick.
    pub fn handle(&mut self, input: Input) {
        self.feed.input(&input);
        self.client.handle(input);
        self.input_since = true;
    }

    /// The display is ready for a frame at `now`, in nanoseconds since the client started.
    /// Draws one if the client or the feed wants it, and says what it cost.
    pub fn tick(&mut self, now: u64, draw: &mut impl Draw) -> Option<Ticked> {
        if !self.wanted(now) {
            self.last = None;
            return None;
        }
        let clock = self.clock(now);
        self.feed.before(&mut self.client, clock);
        let started = Instant::now();
        let frame = self.client.frame(clock);
        let client = started.elapsed().as_nanos() as u64;
        let shown = draw.draw(frame);
        let ticked = Ticked {
            tick: now,
            clock,
            since_last: self.last.map(|last| now.saturating_sub(last)),
            client,
            shown,
        };
        self.last = Some(now);
        self.input_since = false;
        if self.is_paused(now) {
            self.drawn_at_pause = true;
        }
        self.feed.after(&mut self.client, &ticked);
        Some(ticked)
    }

    /// Called when the event loop has handled every waiting event and is about to sleep: runs
    /// ticks while a frame is wanted at `now`, and says how long the loop may sleep.
    pub fn schedule(&mut self, now: u64) -> Sleep {
        let wanted = self.wanted(now);
        self.ticks.set_running(wanted);
        if !wanted {
            self.last = None;
        }
        match self.wake(now) {
            Wake::At(at) if at > now => Sleep::Until(at),
            _ => Sleep::UntilEvent,
        }
    }

    /// The client, for draining what it asks of the platform after a frame.
    pub fn client(&mut self) -> &mut Client {
        &mut self.client
    }

    pub fn ticks(&mut self) -> &mut T {
        &mut self.ticks
    }

    /// The client's clock at `now`: `now`, or the pause once it has passed.
    fn clock(&self, now: u64) -> u64 {
        self.pause.map_or(now, |pause| now.min(pause))
    }

    fn is_paused(&self, now: u64) -> bool {
        self.pause.is_some_and(|pause| now >= pause)
    }

    /// When the next frame is wanted, from the client's wake and the feed's, with the clock
    /// stopped at the pause.
    fn wake(&self, now: u64) -> Wake {
        if self.is_paused(now) {
            return if self.drawn_at_pause && !self.input_since {
                Wake::Never
            } else {
                Wake::Now
            };
        }
        let wake = earlier(self.client.wake(), self.feed.wake());
        match (wake, self.pause) {
            // The clock stops at the pause, so a wait past it ends there.
            (Wake::At(at), Some(pause)) if at > pause => Wake::At(pause),
            (Wake::Never, Some(pause)) if !self.drawn_at_pause => Wake::At(pause),
            _ => wake,
        }
    }

    /// Whether a frame is wanted at `now`.
    fn wanted(&self, now: u64) -> bool {
        match self.wake(now) {
            Wake::Now => true,
            Wake::At(at) => at <= now,
            Wake::Never => false,
        }
    }
}

/// The sooner of two wakes.
fn earlier(first: Wake, second: Wake) -> Wake {
    match (first, second) {
        (Wake::Now, _) | (_, Wake::Now) => Wake::Now,
        (Wake::At(a), Wake::At(b)) => Wake::At(a.min(b)),
        (Wake::At(at), Wake::Never) | (Wake::Never, Wake::At(at)) => Wake::At(at),
        (Wake::Never, Wake::Never) => Wake::Never,
    }
}
