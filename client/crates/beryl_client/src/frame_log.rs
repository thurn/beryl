//! The frame log (docs/beryl-3d.md, "Driving the running app"): every run of the app writes a
//! file of its own, `$BERYL_CACHE/b3d/frames/<start time>-<process id>.jsonl`, that an agent
//! reads with `just client frames`. Its first line describes the run. On macOS, the display
//! link and renderer supply timing windows with submissions, skips and changes of rate or tier.
//! Every host records each late frame, with its timing record and the one before it where the
//! host keeps a trace. The log also records inputs, the first frame and the tier chosen at
//! startup. It summarizes the run every `SUMMARY_EVERY` and when the run ends, including from
//! winit's `exiting` callback on Cmd+Q, after which the process ends without returning, so a
//! reader uses the last summary.
//!
//! A frame is late when more than one and a half intervals of the rate the host asked for
//! passed since the frame before it while the ticks ran (decision C18). When the host asked
//! for more than 60 Hz, a late frame that still came within one and a half sixtieths of a
//! second was held to 60 Hz, which is counted apart from one that missed 60 Hz: the system
//! showed it at 60 Hz without being asked, a fault and not a step down. The host's steps down
//! and up (`beryl_host::rate`) are listed in the summary.
//!
//! The frame thread does no file I/O (docs/beryl-3d.md, "Threads"): it counts frames itself
//! and sends each line to a thread of the log's own, which writes it.

use kit_ui::input::Input as DeviceInput;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::frame_trace::{Reason, Trace, Window};
pub use crate::frame_writer::Attachment;
use crate::frame_writer::{self, Message};
use app_client::Input;
use beryl_host::Ticked;
use beryl_host::rate::Step;
use beryl_host::timing::{FrameTiming, PacingChange, Timing};
use game_play::sound::NAMES;
use kit_frame::frame::SoundId;
use kit_ui::input::{Lifecycle, Modifiers, TextChange};
use serde::{Deserialize, Serialize};

pub use crate::tier::Tiered;

/// A sixtieth of a second, in nanoseconds.
const SIXTIETH: f64 = 1e9 / 60.0;
/// How many intervals may pass between two frames before the second is late.
const LATE: f64 = 1.5;
/// The width of one bucket of the waits for a drawable, in nanoseconds: a tenth of a
/// millisecond.
const WAIT_BUCKET: u64 = 100_000;
/// How many buckets the waits are counted in. The last holds every wait of 100 ms or more.
const WAIT_BUCKETS: usize = 1001;
/// How often a run under way writes a summary.
const SUMMARY_EVERY: Duration = Duration::from_secs(30);

/// The first line: what the run was.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Run {
    /// The checkout the app was built from.
    pub checkout: String,
    pub pid: u32,
    /// When the run started, in milliseconds since the Unix epoch.
    pub started_ms: u64,
    /// The window mode (`none`, `background`, `foreground` or `fullscreen`).
    pub window: String,
    /// The display's highest refresh rate, the rate of the ticks.
    pub display_hz: f32,
    /// The GPU adapter's name.
    pub adapter: String,
    /// The machine's load averages over 1, 5 and 15 minutes, where the system gives them.
    pub load: Option<[f64; 3]>,
    /// The recording replayed instead of a game, if one was.
    pub fixture: Option<String>,
    /// Where the clock stopped, in milliseconds, if it did.
    pub pause_at_ms: Option<f64>,
    /// The drive script played, if one was (`--drive`).
    #[serde(default)]
    pub drive: Option<String>,
    /// The busy threads the app started, as `--load` wrote them (`16:default`), if it did.
    #[serde(default)]
    pub load_threads: Option<String>,
    /// How long each finished frame was held back, in milliseconds, if it was (`--slow-gpu`).
    #[serde(default)]
    pub slow_gpu_ms: Option<f64>,
    /// The game `--agent` started, if it did: who plays it, its seed and the AI's scheduling
    /// class (`expert against mcts-hidden, seed 20261114, search class default`).
    #[serde(default)]
    pub game: Option<String>,
}

/// A late frame. Times are in milliseconds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Late {
    /// When its tick came, since the run started.
    pub at_ms: f64,
    /// The time since the frame before it.
    pub since_last_ms: f64,
    /// The wait for a drawable: the window server's time, not the app's.
    pub waited_ms: f64,
    /// `Client::frame`'s time.
    pub client_ms: f64,
    /// Encoding and submitting its GPU commands, and presenting.
    pub encoded_ms: f64,
    /// The GPU's time, where it was measured. No host measures it yet.
    pub gpu_ms: Option<f64>,
    /// The verbs of the script playing.
    pub verbs: Vec<String>,
    /// Whether the AI was searching.
    pub searching: bool,
    /// Whether it still came within one and a half sixtieths of a second, when the host had
    /// asked for more than 60 Hz.
    pub held_to_60: bool,
    /// The rate the host had asked for, which it was late against.
    #[serde(default)]
    pub hz: f32,
    /// The frame's timing record and the one before it, as a timing window writes them, on a
    /// host that keeps a trace. `just client frames` finds the frame's cause from these.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<FrameTiming>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<FrameTiming>,
}

/// What the run did at one rate the host asked for.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AtRate {
    pub hz: f32,
    /// The time between frames drawn at this rate, while the ticks ran.
    pub seconds: f64,
    pub frames: u64,
    pub late: u64,
}

/// A step the host took (`beryl_host::rate`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stepped {
    /// When, in milliseconds since the run started.
    pub at_ms: f64,
    /// The rate it asked for from then on, or `None` when it dropped the tier to Baseline.
    pub hz: Option<f32>,
    /// How many frames the run had drawn when it stepped, counting the one that made it.
    #[serde(default)]
    pub frame: u64,
}

/// The last line: what the run did.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    /// How long the run lasted from when the host opened, in seconds.
    pub seconds: f64,
    /// Frames drawn. The host draws only on display ticks, one at most per tick.
    pub frames: u64,
    /// Frames drawn on a tick that found no drawable, so nothing was shown.
    pub skipped: u64,
    /// When the first frame was shown, in milliseconds since the run started.
    pub first_shown_ms: Option<f64>,
    pub late: u64,
    /// Late frames that came within one and a half sixtieths of a second, when the host had
    /// asked for more than 60 Hz.
    pub held_to_60: u64,
    /// Late frames that missed 60 Hz too.
    pub missed_60: u64,
    /// The longest time between two frames while the ticks ran, in milliseconds.
    pub worst_gap_ms: f64,
    /// The longest and the total wait for a drawable, in milliseconds.
    pub worst_wait_ms: f64,
    pub total_wait_ms: f64,
    /// The median wait for a drawable over the frames shown, and the 95th percentile, in
    /// milliseconds, to a tenth of a millisecond.
    #[serde(default)]
    pub median_wait_ms: f64,
    #[serde(default)]
    pub p95_wait_ms: f64,
    /// Frames drawn after the one at the pause, when the clock stopped.
    pub after_pause: u64,
    /// The verbs the client played as instant changes because they are not built yet.
    pub unbuilt: Vec<String>,
    /// How many script steps the client's table could not take, each of which made the step
    /// after it place the whole table, so that cards jumped to their places instead of moving.
    #[serde(default)]
    pub unplaced: usize,
    /// As of the last frame, how many named face-up cards were drawn blank because their face
    /// had no layer (`Frame::faceless_cards`), and how many glyphs the text asked for that no
    /// atlas page held (`Client::missing_glyphs`). A host that delivers its card faces and
    /// glyphs leaves both at zero soon after a game's opening hand, which `just client stand-in`
    /// checks. `just client app --for` fails a run that ends with glyphs missing.
    #[serde(default)]
    pub faceless_cards: usize,
    #[serde(default)]
    pub missing_glyphs: usize,
    /// The rate the host asked for when the run ended, in frames a second.
    #[serde(default)]
    pub hz: f32,
    /// The host's steps down and up, in order.
    #[serde(default)]
    pub steps: Vec<Stepped>,
    /// Each rate the host asked for, in the order it first asked for it.
    #[serde(default)]
    pub rates: Vec<AtRate>,
    /// The frames drawn while the latest view said the AI was searching.
    #[serde(default)]
    pub searching: Searching,
    /// How many times each sound was sent to the audio thread, by its name in
    /// `data/audio/manifest.json`. A run with no window sends them to a silent output.
    #[serde(default)]
    pub sounds: BTreeMap<String, u32>,
}

/// The frames drawn while the AI searched: whether its search threads delayed drawing
/// (docs/beryl-3d.md, "Threads"). Times are in milliseconds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Searching {
    pub frames: u64,
    pub late: u64,
    /// The longest `Client::frame` took, and the longest encoding, submitting and presenting took.
    pub worst_client_ms: f64,
    pub worst_encoded_ms: f64,
}

/// One line of the log.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Line {
    Run(Run),
    /// Boxed, being several times the size of the other lines with its timing records.
    Late(Box<Late>),
    FirstShownMs(f64),
    /// The first presentation after resume, or any presentation while suspended.
    LifecycleShownMs(f64),
    Summary(Summary),
    Input(Inputted),
    Tier(Tiered),
    TimingWindow(Window),
    PacingChange(PacingChange),
}

/// An input line as written: the input, and how long before it the last frame was drawn.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Inputted {
    #[serde(flatten)]
    pub input: InputLine,
    /// Milliseconds since the frame before this input was drawn, or none when no frame had
    /// been. A phone smoke test reads it on the last input to prove the host stopped asking for
    /// frames while nothing moved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_frame_ms: Option<f64>,
}

/// An input a host handed the client, as a line `{"input": {"<kind>": {...}}}`. The phone
/// hosts' smoke tests read these to prove their events arrived. Enumerated values are written
/// as their Rust names.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputLine {
    /// Runtime data was read and validated after an edit.
    DataReloaded {},
    /// Runtime data failed to load. The client keeps the last valid tuning.
    DataReloadFailed {
        error: String,
    },
    Pointer {
        id: u8,
        kind: String,
        phase: String,
        /// Logical points from the surface's top left.
        x: f32,
        y: f32,
        button: String,
        wheel_x: f32,
        wheel_y: f32,
        modifiers: Vec<String>,
    },
    Key {
        code: String,
        phase: String,
        modifiers: Vec<String>,
    },
    /// Text typed into the edited field, or its new contents, as its length in characters:
    /// the text itself may be private.
    Text {
        length: usize,
    },
    /// The surface's size and safe-area insets in logical points, and its pixels per point.
    Surface {
        width: f32,
        height: f32,
        scale: f32,
        #[serde(default)]
        top: f32,
        #[serde(default)]
        right: f32,
        #[serde(default)]
        bottom: f32,
        #[serde(default)]
        left: f32,
    },
    Lifecycle {
        event: String,
    },
}

impl InputLine {
    /// The line for `input`, or `None` for an input the log does not record (views, assets,
    /// replies).
    #[must_use]
    pub fn of(input: &Input) -> Option<Self> {
        Some(match input {
            Input::Data(_) => Self::DataReloaded {},
            Input::DataError(error) => Self::DataReloadFailed {
                error: error.clone(),
            },
            Input::Device(DeviceInput::Pointer(pointer)) => Self::Pointer {
                id: pointer.id,
                kind: format!("{:?}", pointer.kind),
                phase: format!("{:?}", pointer.phase),
                x: pointer.position.x,
                y: pointer.position.y,
                button: format!("{:?}", pointer.button),
                wheel_x: pointer.wheel.x,
                wheel_y: pointer.wheel.y,
                modifiers: modifier_names(pointer.modifiers),
            },
            Input::Device(DeviceInput::Key(key)) => Self::Key {
                code: format!("{:?}", key.code),
                phase: format!("{:?}", key.phase),
                modifiers: modifier_names(key.modifiers),
            },
            Input::Device(DeviceInput::Text(
                TextChange::Replace(text) | TextChange::Insert(text) | TextChange::Preedit(text),
            )) => Self::Text {
                length: text.chars().count(),
            },
            Input::Device(DeviceInput::Surface {
                size,
                scale,
                safe_area,
            }) => Self::Surface {
                width: size.width,
                height: size.height,
                scale: *scale,
                top: safe_area.top,
                right: safe_area.right,
                bottom: safe_area.bottom,
                left: safe_area.left,
            },
            Input::Device(DeviceInput::Lifecycle(lifecycle)) => Self::Lifecycle {
                event: format!("{lifecycle:?}"),
            },
            _ => return None,
        })
    }
}

/// The names of the modifier keys held.
fn modifier_names(modifiers: Modifiers) -> Vec<String> {
    [
        (modifiers.shift, "Shift"),
        (modifiers.control, "Control"),
        (modifiers.alt, "Alt"),
        (modifiers.command, "Command"),
    ]
    .into_iter()
    .filter(|(held, _)| *held)
    .map(|(_, name)| name.to_owned())
    .collect()
}

/// A log as `read` found it.
#[derive(Clone, Debug, PartialEq)]
pub struct Log {
    pub run: Run,
    pub late: Vec<Late>,
    /// The inputs the host handed the client, in order.
    pub inputs: Vec<InputLine>,
    /// None when the run ended without writing one (it was killed, say).
    pub summary: Option<Summary>,
    /// The tier chosen at startup, where the host chose one.
    pub tier: Option<Tiered>,
    pub timing_windows: Vec<Window>,
    pub pacing_changes: Vec<PacingChange>,
}

/// The log of a run under way.
pub struct FrameLog {
    /// Where lines go to be written.
    lines: Sender<Message>,
    trace: Option<Trace>,
    writer: JoinHandle<io::Result<()>>,
    path: PathBuf,
    /// When the log was made, as the host opened.
    created: Instant,
    pause_at: Option<u64>,
    summary: Summary,
    /// Whether a frame has been drawn at or after the pause.
    reached_pause: bool,
    /// Suspension, or a resume still waiting for its first presented frame.
    presentation_lifecycle: Option<Lifecycle>,
    /// When the last frame was drawn.
    last_frame: Option<Instant>,
    /// How long after `created` the next summary is due.
    next_summary: Duration,
    /// A late frame waiting for its timing record, which the host notes after the frame.
    pending: Option<Box<Late>>,
    /// The summary written when the run ended, once it has.
    exited: Option<Summary>,
    /// How many shown frames waited for a drawable for each tenth of a millisecond, made once
    /// so that counting a frame allocates nothing.
    waits: Box<[u32; WAIT_BUCKETS]>,
    /// How many times each sound was played, by its id, made once so that counting a sound
    /// allocates nothing.
    sounds: Box<[u32]>,
}

impl FrameLog {
    /// Starts a log for `run` in `dir`, named for its start time and process. The log's own
    /// thread creates the file and writes the first line, since the host calls this on the
    /// frame thread.
    ///
    /// # Errors
    ///
    /// When the log's thread cannot start. A file that cannot be created or written is reported
    /// by `finish`.
    pub fn create(dir: &Path, run: &Run) -> io::Result<Self> {
        let path = dir.join(format!("{}-{}.jsonl", run.started_ms, run.pid));
        let (lines, received) = channel();
        let (dir, file_path, first) = (dir.to_owned(), path.clone(), Line::Run(run.clone()));
        let writer = std::thread::Builder::new()
            .name("frame log".to_owned())
            .spawn(move || frame_writer::run(&dir, &file_path, &first, received))?;
        Ok(Self {
            lines,
            trace: None,
            writer,
            path,
            created: Instant::now(),
            pause_at: run.pause_at_ms.map(|ms| (ms * 1e6) as u64),
            summary: Summary::default(),
            reached_pause: false,
            presentation_lifecycle: None,
            last_frame: None,
            next_summary: SUMMARY_EVERY,
            pending: None,
            exited: None,
            waits: Box::new([0; WAIT_BUCKETS]),
            sounds: vec![0; NAMES.len()].into_boxed_slice(),
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn send(&self, line: Line) {
        let _ = self.lines.send(Message::Line(line));
    }

    /// Starts keeping a trace of the host's timing records on the clock that starts at
    /// `epoch`.
    pub fn start_trace(&mut self, epoch: Instant) {
        self.trace = Some(Trace::new(epoch));
    }

    /// Records one of the host's timing records. A frame's record completes the late line
    /// `frame` held back for it.
    pub fn timing(&mut self, event: &Timing) {
        if let Timing::Change(change) = event {
            self.send(Line::PacingChange(*change));
        }
        let Some(trace) = &mut self.trace else {
            return;
        };
        let mut windows = [trace.record(event), None];
        let mut late = None;
        if let Timing::Frame(frame) = event
            && let Some(mut line) = self.pending.take()
        {
            (line.previous, line.frame) = trace.last_two_frames();
            windows[1] = trace.late(frame.finished_ns);
            late = Some(line);
        }
        for window in windows.into_iter().flatten() {
            self.send(Line::TimingWindow(window));
        }
        if let Some(late) = late {
            self.send(Line::Late(late));
        }
    }

    /// Writes the late line still waiting for its timing record without one.
    fn send_pending(&mut self) {
        if let Some(late) = self.pending.take() {
            self.send(Line::Late(late));
        }
    }

    pub(crate) fn freeze_trace(&mut self) {
        if let Some(trace) = &mut self.trace {
            trace.frozen = trace.capture(Reason::BugOpened, trace.now());
        }
    }

    pub(crate) fn bug_attachment(&mut self) -> Attachment {
        if let Some(trace) = &mut self.trace {
            let frozen = trace.frozen.take();
            let latest = trace.capture(Reason::BugSaved, trace.now());
            for window in frozen.into_iter().chain(latest) {
                self.send(Line::TimingWindow(window));
            }
        }
        let (finished, complete) = channel();
        let _ = self.lines.send(Message::Written(finished));
        Attachment::new(&self.path, complete)
    }

    /// Whether `ticked` came late against the rate the host had asked for.
    #[must_use]
    pub fn is_late(ticked: &Ticked) -> bool {
        ticked
            .since_last
            .is_some_and(|gap| gap as f64 > LATE * 1e9 / f64::from(ticked.hz.max(1.0)))
    }

    /// Counts a frame the host drew, and sends a line for it to be written when it was late.
    pub fn frame(&mut self, ticked: &Ticked, verbs: &[&str], searching: bool) {
        let now = Instant::now();
        self.last_frame = Some(now);
        self.send_pending();
        if now.duration_since(self.created) >= self.next_summary {
            self.next_summary += SUMMARY_EVERY;
            self.summarize();
        }
        let late = Self::is_late(ticked);
        self.count_rate(ticked, late);
        let summary = &mut self.summary;
        summary.frames += 1;
        summary.hz = ticked.hz;
        if let Some(step) = ticked.step {
            summary.steps.push(Stepped {
                at_ms: ms(ticked.tick),
                hz: match step {
                    Step::Baseline => None,
                    Step::Rate(hz) => Some(hz),
                },
                frame: summary.frames,
            });
        }
        if let Some(pause) = self.pause_at
            && ticked.tick >= pause
        {
            if self.reached_pause {
                summary.after_pause += 1;
            }
            self.reached_pause = true;
        }
        if searching {
            summary.searching.frames += 1;
            summary.searching.worst_client_ms =
                summary.searching.worst_client_ms.max(ms(ticked.client));
        }
        let Some(shown) = ticked.shown else {
            summary.skipped += 1;
            return;
        };
        if searching {
            summary.searching.worst_encoded_ms =
                summary.searching.worst_encoded_ms.max(ms(shown.work()));
        }
        if summary.first_shown_ms.is_none() {
            let at = ms(ticked.tick);
            summary.first_shown_ms = Some(at);
            let _ = self.lines.send(Message::Line(Line::FirstShownMs(at)));
        }
        if self.presentation_lifecycle.is_some() {
            let _ = self
                .lines
                .send(Message::Line(Line::LifecycleShownMs(ms(ticked.tick))));
            if self.presentation_lifecycle == Some(Lifecycle::Resumed) {
                self.presentation_lifecycle = None;
            }
        }
        let waited = ms(shown.waited);
        let bucket = usize::try_from(shown.waited / WAIT_BUCKET).unwrap_or(usize::MAX);
        self.waits[bucket.min(WAIT_BUCKETS - 1)] += 1;
        summary.worst_wait_ms = summary.worst_wait_ms.max(waited);
        summary.total_wait_ms += waited;
        let Some(gap) = ticked.since_last else {
            return;
        };
        summary.worst_gap_ms = summary.worst_gap_ms.max(ms(gap));
        if !late {
            return;
        }
        let held_to_60 = ticked.hz > 61.0 && (gap as f64) <= LATE * SIXTIETH;
        summary.late += 1;
        if searching {
            summary.searching.late += 1;
        }
        if held_to_60 {
            summary.held_to_60 += 1;
        } else {
            summary.missed_60 += 1;
        }
        let late = Box::new(Late {
            at_ms: ms(ticked.tick),
            since_last_ms: ms(gap),
            waited_ms: waited,
            client_ms: ms(ticked.client),
            encoded_ms: ms(shown.work()),
            gpu_ms: None,
            verbs: verbs.iter().map(|verb| (*verb).to_owned()).collect(),
            searching,
            held_to_60,
            hz: ticked.hz,
            frame: None,
            previous: None,
        });
        // With a trace, the line waits for the frame's timing record, which comes next.
        if self.trace.is_some() {
            self.pending = Some(late);
        } else {
            self.send(Line::Late(late));
        }
    }

    /// Counts a frame against the rate the host asked for when it came. A rate seen for the
    /// first time allocates, which happens only at a step.
    fn count_rate(&mut self, ticked: &Ticked, late: bool) {
        let rates = &mut self.summary.rates;
        // The host asks only for rates that divide the display's highest, so the same rate is
        // always the same number.
        let index = rates
            .iter()
            .position(|rate| rate.hz.to_bits() == ticked.hz.to_bits())
            .unwrap_or_else(|| {
                rates.push(AtRate {
                    hz: ticked.hz,
                    ..AtRate::default()
                });
                rates.len() - 1
            });
        let rate = &mut rates[index];
        rate.frames += 1;
        rate.late += u64::from(late);
        rate.seconds += ticked.since_last.map_or(0.0, |gap| gap as f64 / 1e9);
    }

    /// Records an input the host handed the client when it is one the log keeps, with how
    /// long ago the last frame was drawn.
    pub fn input(&mut self, input: &Input) {
        if let Input::Device(DeviceInput::Lifecycle(
            event @ (Lifecycle::Suspended | Lifecycle::Resumed),
        )) = input
        {
            self.presentation_lifecycle = Some(*event);
        }
        if let Some(line) = InputLine::of(input) {
            let since_frame_ms = self
                .last_frame
                .map(|drawn| drawn.elapsed().as_secs_f64() * 1e3);
            // The writer ends only with the log, so a failed send has nowhere to go.
            self.send(Line::Input(Inputted {
                input: line,
                since_frame_ms,
            }));
        }
    }

    /// Writes the tier chosen at startup.
    pub fn tier(&self, tiered: Tiered) {
        // The writer ends only with the log, so a failed send has nowhere to go.
        self.send(Line::Tier(tiered));
    }

    /// Notes a sound the client asked for in a frame.
    pub fn sound(&mut self, id: SoundId) {
        if let Some(count) = self.sounds.get_mut(usize::from(id.0)) {
            *count += 1;
        }
    }

    /// Notes a verb the client played as an instant change.
    pub fn unbuilt(&mut self, verb: &str) {
        self.summary.unbuilt.push(verb.to_owned());
    }

    /// Notes how many cards the last frame drew blank and how many glyphs its text lacked.
    pub fn assets(&mut self, faceless_cards: usize, missing_glyphs: usize) {
        self.summary.faceless_cards = faceless_cards;
        self.summary.missing_glyphs = missing_glyphs;
    }

    /// Notes how many script steps the client's table has failed to take so far.
    pub fn unplaced(&mut self, steps: usize) {
        self.summary.unplaced = steps;
    }

    /// Writes a summary while keeping the log open for input after the replay ends.
    pub fn summarize(&mut self) {
        let summary = self.summary();
        self.send(Line::Summary(summary));
    }

    fn summary(&mut self) -> Summary {
        self.summary.seconds = self.created.elapsed().as_secs_f64();
        let worst = self.summary.worst_wait_ms;
        self.summary.median_wait_ms = percentile(&self.waits[..], 0.5).min(worst);
        self.summary.p95_wait_ms = percentile(&self.waits[..], 0.95).min(worst);
        self.summary.sounds = (NAMES.iter().zip(&self.sounds))
            .filter(|(_, count)| **count > 0)
            .map(|(name, count)| ((*name).to_owned(), *count))
            .collect();
        self.summary.clone()
    }

    /// Writes the `exit` timing window and the summary, and waits until the writer has written
    /// them, since a host may end the process as soon as this returns: macOS does after winit's
    /// `exiting`. Later frames are not counted. Does nothing the second time.
    pub fn exit(&mut self) {
        if self.exited.is_some() {
            return;
        }
        self.send_pending();
        if let Some(trace) = &self.trace
            && let Some(window) = trace.capture(Reason::Exit, trace.now())
        {
            self.send(Line::TimingWindow(window));
        }
        let summary = self.summary();
        self.send(Line::Summary(summary.clone()));
        self.exited = Some(summary);
        let (written, complete) = channel();
        let _ = self.lines.send(Message::Written(written));
        // A writer that failed has dropped the message, so this returns at once.
        let _ = complete.recv();
    }

    /// Ends the log, written by `exit` if it was not already, and returns its summary.
    ///
    /// # Errors
    /// When a line could not be written.
    pub fn finish(mut self) -> io::Result<Summary> {
        self.exit();
        let summary = self.exited.take().unwrap_or_default();
        drop(self.lines);
        self.writer
            .join()
            .map_err(|_| io::Error::other("the frame log's writer panicked"))??;
        Ok(summary)
    }
}

/// The wait, in milliseconds, that a share `q` of the counted frames waited no longer than: the
/// top of the bucket the frame at that rank falls in. Zero when no frame was counted.
fn percentile(waits: &[u32], q: f64) -> f64 {
    let total: u64 = waits.iter().map(|&count| u64::from(count)).sum();
    if total == 0 {
        return 0.0;
    }
    let rank = ((total as f64 * q).ceil() as u64).max(1);
    let mut seen = 0;
    for (bucket, &count) in waits.iter().enumerate() {
        seen += u64::from(count);
        if seen >= rank {
            return ms((bucket as u64 + 1) * WAIT_BUCKET);
        }
    }
    ms(waits.len() as u64 * WAIT_BUCKET)
}

mod read;
pub use read::read;
pub(crate) use read::read_run;

/// Converts nanoseconds to milliseconds for the legacy log.
fn ms(nanoseconds: u64) -> f64 {
    nanoseconds as f64 / 1e6
}
