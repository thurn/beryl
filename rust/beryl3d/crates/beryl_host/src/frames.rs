//! The frame loop every host runs, apart from any window (docs/beryl-3d.md, "Hosts" and
//! "Drawing only when something changes"). An input reaches the client at once but is drawn
//! only on the next tick of the display, and ticks are asked for only while the client wants a
//! frame. Where ticks come from is a type parameter, so a test can supply them.

use std::time::Instant;

use beryl_render::{Picture, Readback};
use beryl_scene::Client;
use beryl_scene::Input;
use beryl_scene::command::Request;
use beryl_scene::frame::{Frame, Wake};
use beryl_scene::input::Tier;
use beryl_scene::settings::Graphics;

use crate::rate::{Paced, RateRule, Step, Thresholds};

/// A source of display ticks: a display link on macOS, winit's redraw requests elsewhere, a
/// timer in the windowless host, or a test's own calls.
pub trait Ticks {
    /// Starts ticks while `running` is true and stops them otherwise. Called after every batch
    /// of events, so a source may ask again on each call while running.
    fn set_running(&mut self, running: bool);

    /// Asks the display for `hz` frames a second, a rate that divides its highest
    /// (`crate::rate`). A source that cannot change its rate ignores it, and its host counts
    /// frames against a fixed rate instead (`FrameLoop::fixed_at`).
    fn set_rate(&mut self, _hz: f32) {}
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

    /// Starts reading back what drawing `frame` showed, for a feed that asked for a picture
    /// (`Feed::capture_wanted`). A host that cannot returns `None`.
    fn capture(&mut self, _frame: &Frame) -> Option<Readback> {
        None
    }

    /// Hands what `client` wants of the platform's assets (glyphs and card faces) to the
    /// host's workers, and gives `client` what they have finished. Called before each frame,
    /// so that the frame draws what has arrived, and after it, so that what the frame found
    /// missing is asked for even when the client then wants no more frames. It never waits; a
    /// worker that finishes something wakes the host. A host with no workers does nothing.
    fn assets(&mut self, _client: &mut Client, _when: When) {}
}

/// Which of a frame's two calls to `Draw::assets` this is. Uploads limited to so many a frame
/// (`Renderer::upload_waiting`) are made only before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    BeforeFrame,
    AfterFrame,
}

/// One frame the loop drew.
#[derive(Clone, Copy, Debug, PartialEq)]
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
    /// The rate the host had asked the display for when the tick came, in frames a second. A
    /// frame is late against this rate.
    pub hz: f32,
    /// The step the host took after this frame, if it took one.
    pub step: Option<Step>,
}

/// What the host found when it opened its window or target, for the app's log.
#[derive(Clone, Debug, PartialEq)]
pub struct Opened {
    /// The display's highest refresh rate, in frames a second: the rate of the ticks.
    pub display_hz: f32,
    /// The GPU adapter's name.
    pub adapter: String,
}

/// What a host has opened, for the app to prepare the renderer before the first frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Opening {
    /// The size of the window or target, in physical pixels.
    pub width: u32,
    pub height: u32,
    /// Physical pixels per logical point.
    pub scale: f32,
    /// The display's highest refresh rate, in frames a second.
    pub display_hz: f32,
}

/// The tier a host starts the client in, as the app chose it before the first frame
/// (docs/beryl-3d.md, "Choosing a tier"), and what the frame loop needs to change it when the
/// player changes the setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chosen {
    /// How the player asked for the tier to be chosen.
    pub graphics: Graphics,
    /// The tier the client starts in.
    pub tier: Tier,
    /// Whether the renderer can draw the Enhanced tier (`Renderer::enhanced_available`).
    pub enhanced_available: bool,
}

impl Chosen {
    /// The Baseline tier, chosen automatically on a renderer without Enhanced: what a host
    /// whose app chose nothing starts in.
    pub const BASELINE: Self = Self {
        graphics: Graphics::Automatic,
        tier: Tier::Baseline,
        enhanced_available: false,
    };

    /// Whether the rate rule may drop the tier to Baseline: it is Enhanced and chosen
    /// automatically.
    #[must_use]
    pub fn tier_can_drop(self) -> bool {
        self.graphics == Graphics::Automatic && self.tier == Tier::Enhanced
    }
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

    /// Sees each of the client's requests before the host carries it out, as the sound
    /// requests that the app plays and the settings it saves.
    fn request(&mut self, _request: &Request) {}

    /// When the feed next needs a frame, beside the client's own wake.
    fn wake(&self) -> Wake {
        Wake::Never
    }

    /// Whether the feed wants a picture of the frame just drawn. Asked after every frame.
    fn capture_wanted(&mut self) -> bool {
        false
    }

    /// A picture the feed asked for, once the GPU has finished it: some frames after the one it
    /// shows.
    fn captured(&mut self, _picture: Picture) {}

    /// Scripts of actions to play now, each with an id, on the display's clock from this tick.
    /// Unlike the run's own script, a finished one does not end the run.
    #[cfg(feature = "drive")]
    fn scripts(&mut self) -> Vec<(u64, crate::drive::Drive)> {
        Vec::new()
    }

    /// The script with this id has been played, or stopped with an error.
    #[cfg(feature = "drive")]
    fn script_done(&mut self, _id: u64, _result: Result<(), String>) {}

    /// Whether the feed has nothing more to show, as when the game it plays is over: the host
    /// then ends the run.
    fn ended(&self) -> bool {
        false
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
    /// The rate the host asks the display for, and when it steps (`paced`). Without one the
    /// loop counts frames against `hz` and never steps.
    rate: Option<RateRule>,
    /// The rate frames are counted against: the rule's choice, or the fixed rate.
    hz: f32,
    /// How the tier is chosen, as the settings last said, and whether the renderer has the
    /// Enhanced tier, so that a change of the setting during play can be carried out.
    graphics: Graphics,
    enhanced_available: bool,
    /// The drive script being played, if any (`--drive`).
    #[cfg(feature = "drive")]
    drive: Option<crate::drive::Drive>,
    /// Scripts the feed handed over while the run was going, each with its id.
    #[cfg(feature = "drive")]
    live: Vec<(u64, crate::drive::Drive)>,
    /// Pictures the GPU has not finished yet.
    readbacks: Vec<Readback>,
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
            rate: None,
            hz: 60.0,
            graphics: Graphics::Automatic,
            enhanced_available: false,
            #[cfg(feature = "drive")]
            drive: None,
            #[cfg(feature = "drive")]
            live: Vec::new(),
            readbacks: Vec::new(),
        }
    }

    /// The same loop asking for `display_hz`, the display's highest rate, and stepping down
    /// and up by the rule in `crate::rate` with its thresholds from the data. `tier_can_drop`
    /// says whether the tier is Enhanced and chosen automatically, so that it drops first.
    #[must_use]
    pub fn paced(self, display_hz: f32, tier_can_drop: bool) -> Self {
        self.paced_by(RateRule::new(display_hz, tier_can_drop, Thresholds::DATA))
    }

    /// The same loop stepping by `rule`.
    fn paced_by(mut self, rule: RateRule) -> Self {
        self.hz = rule.hz();
        self.ticks.set_rate(rule.hz());
        self.rate = Some(rule);
        self
    }

    /// The same loop counting frames against `hz`, for a host whose ticks come at a fixed rate
    /// (winit's redraw requests, which cannot ask for a lower one). Its rule still drops the
    /// tier when `tier_can_drop`, but never steps the rate: its lowest rate is `hz` itself.
    #[must_use]
    pub fn fixed_at(self, hz: f32, tier_can_drop: bool) -> Self {
        let thresholds = Thresholds {
            lowest_hz: hz.round() as usize,
            ..Thresholds::DATA
        };
        self.paced_by(RateRule::new(hz, tier_can_drop, thresholds))
    }

    /// The same loop starting the client in `chosen`'s tier, and remembering how it was chosen
    /// so that a change of the setting during play is carried out (`next_request`). The rule's
    /// drop is the host's to set, through `paced` or `fixed_at` with `chosen.tier_can_drop()`.
    #[must_use]
    pub fn with_tier(mut self, chosen: Chosen) -> Self {
        self.graphics = chosen.graphics;
        self.enhanced_available = chosen.enhanced_available;
        self.client.handle(Input::Tier(chosen.tier));
        self
    }

    /// The same loop playing `drive` on the display's clock.
    #[cfg(feature = "drive")]
    #[must_use]
    pub fn driven(mut self, drive: Option<crate::drive::Drive>) -> Self {
        self.drive = drive;
        self
    }

    /// Whether the run is over: the drive script has given its last action, with the error
    /// that stopped it early if one did, or the feed has ended. `None` while neither has
    /// happened. The host ends the run when it is `Some`.
    #[must_use]
    pub fn at_end(&self) -> Option<Result<(), String>> {
        #[cfg(feature = "drive")]
        if let Some(drive) = &self.drive
            && drive.finished()
        {
            return Some(drive.error().map_or(Ok(()), |error| Err(error.to_owned())));
        }
        self.feed.ended().then_some(Ok(()))
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
        self.deliver_pictures();
        #[cfg(feature = "drive")]
        if let Some(drive) = &mut self.drive {
            let mut inputs = Vec::new();
            drive.due(now, clock, &mut self.client, &mut inputs);
            for input in inputs {
                self.handle(input);
            }
        }
        #[cfg(feature = "drive")]
        self.play_live(now, clock);
        self.feed.before(&mut self.client, clock);
        draw.assets(&mut self.client, When::BeforeFrame);
        // What the frame needs of the scripts to come is compiled before it, outside the frame's
        // own time: the frame allocates nothing, and compiling does.
        self.client.prepare(clock);
        let started = Instant::now();
        let frame = self.client.frame(clock);
        let client = started.elapsed().as_nanos() as u64;
        let shown = draw.draw(frame);
        if self.feed.capture_wanted()
            && let Some(readback) = draw.capture(frame)
        {
            self.readbacks.push(readback);
        }
        draw.assets(&mut self.client, When::AfterFrame);
        let since_last = self.last.map(|last| now.saturating_sub(last));
        let hz = self.hz;
        let step = self.pace(since_last, client, shown);
        let ticked = Ticked {
            tick: now,
            clock,
            since_last,
            client,
            shown,
            hz,
            step,
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

    /// When the loop next wants a frame, as of `now`: the client's wake, the feed's and the
    /// drive script's together, with the clock stopped at the pause. A host whose platform can
    /// stop its ticks (a phone's display link) asks this, not the client alone, or it would
    /// stop them while a replayed recording still has views to give.
    #[must_use]
    pub fn next_wake(&self, now: u64) -> Wake {
        self.wake(now)
    }

    /// The next thing the client asks of the platform, shown to the feed first. A host drains
    /// these after each frame and carries out the ones it can.
    pub fn next_request(&mut self) -> Option<Request> {
        let request = self.client.next_request()?;
        if let Request::SaveSettings(settings) = &request
            && settings.graphics != self.graphics
        {
            self.set_graphics(settings.graphics);
        }
        self.feed.request(&request);
        Some(request)
    }

    /// Carries out a change of how the tier is chosen during play, with no new measurement
    /// (docs/beryl-3d.md, "Choosing a tier"): Baseline draws Baseline; Enhanced draws Enhanced
    /// where the renderer has it, and never drops; Automatic keeps the tier, and lets it drop
    /// if it is Enhanced.
    fn set_graphics(&mut self, graphics: Graphics) {
        self.graphics = graphics;
        match graphics {
            Graphics::Baseline => self.client.handle(Input::Tier(Tier::Baseline)),
            Graphics::Enhanced if self.enhanced_available => {
                self.client.handle(Input::Tier(Tier::Enhanced));
            }
            Graphics::Enhanced | Graphics::Automatic => {}
        }
        let tier_can_drop = graphics == Graphics::Automatic && self.client.tier() == Tier::Enhanced;
        if let Some(rule) = &mut self.rate {
            rule.set_tier_can_drop(tier_can_drop);
        }
    }

    /// The client, for draining what it asks of the platform after a frame.
    pub fn client(&mut self) -> &mut Client {
        &mut self.client
    }

    pub fn ticks(&mut self) -> &mut T {
        &mut self.ticks
    }

    /// The source of ticks, to read.
    #[must_use]
    pub fn ticks_ref(&self) -> &T {
        &self.ticks
    }

    /// Feeds an animating frame to the rate rule, and carries out the step it returns: a
    /// tier for the client, a rate for the ticks.
    fn pace(&mut self, since_last: Option<u64>, client: u64, shown: Option<Shown>) -> Option<Step> {
        let rule = self.rate.as_mut()?;
        let gap = since_last?;
        let work = client + shown.map_or(0, |shown| shown.encoded);
        let step = rule.frame(Paced { gap, work })?;
        match step {
            Step::Baseline => self.client.handle(Input::Tier(Tier::Baseline)),
            Step::Rate(hz) => {
                self.hz = hz;
                self.ticks.set_rate(hz);
            }
        }
        Some(step)
    }

    /// Gives the feed the pictures the GPU has finished.
    fn deliver_pictures(&mut self) {
        let mut waiting = Vec::new();
        for readback in std::mem::take(&mut self.readbacks) {
            match readback.poll() {
                Some(picture) => self.feed.captured(picture),
                None => waiting.push(readback),
            }
        }
        self.readbacks = waiting;
    }

    /// Takes the scripts the feed has for the run, plays what is due of every one, and tells the
    /// feed which have ended.
    #[cfg(feature = "drive")]
    fn play_live(&mut self, now: u64, clock: u64) {
        for (id, mut script) in self.feed.scripts() {
            script.start_at(now);
            self.live.push((id, script));
        }
        let mut inputs = Vec::new();
        for (_, script) in &mut self.live {
            script.due(now, clock, &mut self.client, &mut inputs);
        }
        for input in inputs {
            self.handle(input);
        }
        let mut playing = Vec::new();
        for (id, script) in std::mem::take(&mut self.live) {
            if script.finished() {
                let result = script.error().map_or(Ok(()), |error| Err(error.to_owned()));
                self.feed.script_done(id, result);
            } else {
                playing.push((id, script));
            }
        }
        self.live = playing;
    }

    /// The client's clock at `now`: `now`, or the pause once it has passed.
    fn clock(&self, now: u64) -> u64 {
        self.pause.map_or(now, |pause| now.min(pause))
    }

    fn is_paused(&self, now: u64) -> bool {
        self.pause.is_some_and(|pause| now >= pause)
    }

    /// When the next frame is wanted, from the client's wake, the feed's and the drive
    /// script's, with the clock stopped at the pause.
    fn wake(&self, now: u64) -> Wake {
        let wake = if self.is_paused(now) {
            if self.drawn_at_pause && !self.input_since {
                Wake::Never
            } else {
                Wake::Now
            }
        } else {
            let wake = earlier(self.client.wake(), self.feed.wake());
            match (wake, self.pause) {
                // The clock stops at the pause, so a wait past it ends there.
                (Wake::At(at), Some(pause)) if at > pause => Wake::At(pause),
                (Wake::Never, Some(pause)) if !self.drawn_at_pause => Wake::At(pause),
                _ => wake,
            }
        };
        // A drive script plays on the display's clock, which runs on through a pause.
        #[cfg(feature = "drive")]
        let wake = {
            let mut wake = wake;
            if let Some(drive) = &self.drive {
                wake = earlier(wake, drive.wake(self.client.wake()));
            }
            for (_, script) in &self.live {
                wake = earlier(wake, script.wake(self.client.wake()));
            }
            wake
        };
        // A picture the GPU has not finished is polled on the next tick.
        if self.readbacks.is_empty() {
            wake
        } else {
            Wake::Now
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
