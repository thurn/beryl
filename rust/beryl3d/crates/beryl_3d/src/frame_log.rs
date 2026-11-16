//! The frame log (docs/beryl-3d.md, "Driving the running app"): every run of the app writes a
//! file of its own, `$BERYL_CACHE/b3d/frames/<start time>-<process id>.jsonl`, that an agent
//! reads with `just b3d frames`. Its first line describes the run, then one line follows for
//! each late frame and for each pointer, key, text, surface and lifecycle input. The log
//! records when the first frame was shown and summarizes the run at exit or at replay end.
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

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};
use std::thread::JoinHandle;
use std::time::Instant;

use beryl_host::Ticked;
use beryl_host::rate::Step;
use beryl_scene::frame::SoundId;
use beryl_scene::input::{Input, Lifecycle, Modifiers, TextChange};
use beryl_scene::sound::NAMES;
use serde::{Deserialize, Serialize};

/// A sixtieth of a second, in nanoseconds.
const SIXTIETH: f64 = 1e9 / 60.0;
/// How many intervals may pass between two frames before the second is late.
const LATE: f64 = 1.5;
/// The width of one bucket of the waits for a drawable, in nanoseconds: a tenth of a
/// millisecond.
const WAIT_BUCKET: u64 = 100_000;
/// How many buckets the waits are counted in. The last holds every wait of 100 ms or more.
const WAIT_BUCKETS: usize = 1001;

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
    /// Encoding and submitting its GPU commands.
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
}

/// A step the host took (`beryl_host::rate`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stepped {
    /// When, in milliseconds since the run started.
    pub at_ms: f64,
    /// The rate it asked for from then on, or `None` when it dropped the tier to Baseline.
    pub hz: Option<f32>,
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
    /// glyphs leaves both at zero soon after a game's opening hand, which `just b3d stand-in`
    /// checks.
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
    /// The frames drawn while the latest view said the AI was searching.
    #[serde(default)]
    pub searching: Searching,
    /// How many times each sound was sent to the audio thread, by its name in
    /// `ui/src/audio/manifest.json`. A run with no window sends them to a silent output.
    #[serde(default)]
    pub sounds: BTreeMap<String, u32>,
}

/// The frames drawn while the AI searched: whether its search threads delayed drawing
/// (docs/beryl-3d.md, "Threads"). Times are in milliseconds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Searching {
    pub frames: u64,
    pub late: u64,
    /// The longest `Client::frame` took, and the longest the encode and submit took.
    pub worst_client_ms: f64,
    pub worst_encoded_ms: f64,
}

/// One line of the log.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Line {
    Run(Run),
    Late(Late),
    FirstShownMs(f64),
    /// The first presentation after resume, or any presentation while suspended.
    LifecycleShownMs(f64),
    Summary(Summary),
    Input(Inputted),
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
            Input::Pointer(pointer) => Self::Pointer {
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
            Input::Key(key) => Self::Key {
                code: format!("{:?}", key.code),
                phase: format!("{:?}", key.phase),
                modifiers: modifier_names(key.modifiers),
            },
            Input::Text(
                TextChange::Replace(text) | TextChange::Insert(text) | TextChange::Preedit(text),
            ) => Self::Text {
                length: text.chars().count(),
            },
            Input::Surface {
                size,
                scale,
                safe_area,
            } => Self::Surface {
                width: size.width,
                height: size.height,
                scale: *scale,
                top: safe_area.top,
                right: safe_area.right,
                bottom: safe_area.bottom,
                left: safe_area.left,
            },
            Input::Lifecycle(lifecycle) => Self::Lifecycle {
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
}

/// The log of a run under way.
pub struct FrameLog {
    /// Where lines go to be written.
    lines: Sender<Line>,
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
        let (lines, received) = channel::<Line>();
        let (dir, file_path, first) = (dir.to_owned(), path.clone(), Line::Run(run.clone()));
        let writer = std::thread::Builder::new()
            .name("frame log".to_owned())
            .spawn(move || {
                beryl_host::utility_thread();
                fs::create_dir_all(&dir)?;
                let mut file = BufWriter::new(File::create(&file_path)?);
                for line in std::iter::once(first).chain(received) {
                    write(&mut file, &line)?;
                    file.flush()?;
                }
                Ok(())
            })?;
        Ok(Self {
            lines,
            writer,
            path,
            created: Instant::now(),
            pause_at: run.pause_at_ms.map(|ms| (ms * 1e6) as u64),
            summary: Summary::default(),
            reached_pause: false,
            presentation_lifecycle: None,
            last_frame: None,
            waits: Box::new([0; WAIT_BUCKETS]),
            sounds: vec![0; NAMES.len()].into_boxed_slice(),
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
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
        self.last_frame = Some(Instant::now());
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
                summary.searching.worst_encoded_ms.max(ms(shown.encoded));
        }
        if summary.first_shown_ms.is_none() {
            let at = ms(ticked.tick);
            summary.first_shown_ms = Some(at);
            let _ = self.lines.send(Line::FirstShownMs(at));
        }
        if self.presentation_lifecycle.is_some() {
            let _ = self.lines.send(Line::LifecycleShownMs(ms(ticked.tick)));
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
        if !Self::is_late(ticked) {
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
        let late = Late {
            at_ms: ms(ticked.tick),
            since_last_ms: ms(gap),
            waited_ms: waited,
            client_ms: ms(ticked.client),
            encoded_ms: ms(shown.encoded),
            gpu_ms: None,
            verbs: verbs.iter().map(|verb| (*verb).to_owned()).collect(),
            searching,
            held_to_60,
            hz: ticked.hz,
        };
        // The writer ends only with the log, so a failed send has nowhere to go.
        let _ = self.lines.send(Line::Late(late));
    }

    /// Records an input the host handed the client when it is one the log keeps, with how
    /// long ago the last frame was drawn.
    pub fn input(&mut self, input: &Input) {
        if let Input::Lifecycle(event @ (Lifecycle::Suspended | Lifecycle::Resumed)) = input {
            self.presentation_lifecycle = Some(*event);
        }
        if let Some(line) = InputLine::of(input) {
            let since_frame_ms = self
                .last_frame
                .map(|drawn| drawn.elapsed().as_secs_f64() * 1e3);
            // The writer ends only with the log, so a failed send has nowhere to go.
            let _ = self.lines.send(Line::Input(Inputted {
                input: line,
                since_frame_ms,
            }));
        }
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
        let _ = self.lines.send(Line::Summary(summary));
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

    /// Writes the summary as the last line and returns it.
    ///
    /// # Errors
    /// When a line could not be written.
    pub fn finish(mut self) -> io::Result<Summary> {
        let summary = self.summary();
        let _ = self.lines.send(Line::Summary(summary.clone()));
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

fn write(file: &mut BufWriter<File>, line: &Line) -> io::Result<()> {
    serde_json::to_writer(&mut *file, line)?;
    file.write_all(b"\n")
}

/// Reads the log at `path`.
///
/// # Errors
///
/// When the file cannot be read, a line is not a log line, or the first line does not
/// describe a run.
pub fn read(path: &Path) -> Result<Log, String> {
    let file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut lines = BufReader::new(file)
        .lines()
        .enumerate()
        .map(|(index, line)| {
            let line = line.map_err(|error| format!("{}: {error}", path.display()))?;
            serde_json::from_str::<Line>(&line)
                .map_err(|error| format!("{}:{}: {error}", path.display(), index + 1))
        });
    let Some(Line::Run(run)) = lines.next().transpose()? else {
        return Err(format!(
            "{}: the first line does not describe a run",
            path.display()
        ));
    };
    let mut log = Log {
        run,
        late: Vec::new(),
        inputs: Vec::new(),
        summary: None,
    };
    for line in lines {
        match line? {
            Line::Run(_) => return Err(format!("{}: a second run line", path.display())),
            Line::FirstShownMs(_) | Line::LifecycleShownMs(_) => {}
            Line::Late(late) => log.late.push(late),
            Line::Summary(summary) => log.summary = Some(summary),
            Line::Input(input) => log.inputs.push(input.input),
        }
    }
    Ok(log)
}

impl Run {
    /// The machine's load as the run started and what the run was made to imitate, one line:
    /// its drive script, its busy threads and its slowed frames. `None` when there is nothing to
    /// say.
    #[must_use]
    pub fn conditions(&self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some([one, five, fifteen]) = self.load {
            parts.push(format!("load average {one:.1} {five:.1} {fifteen:.1}"));
        }
        if let Some(drive) = &self.drive {
            parts.push(format!("drive {drive}"));
        }
        if let Some(load) = &self.load_threads {
            parts.push(format!("load threads {load}"));
        }
        if let Some(slow) = self.slow_gpu_ms {
            parts.push(format!("slow GPU {slow} ms"));
        }
        if let Some(game) = &self.game {
            parts.push(format!("game {game}"));
        }
        (!parts.is_empty()).then(|| parts.join(", "))
    }
}

impl Summary {
    /// The rate the host asked for, and its steps, one line.
    fn print_rate(&self, run: &Run) -> String {
        if self.steps.is_empty() {
            return format!("asked for {} Hz throughout", run.display_hz);
        }
        let steps: Vec<String> = self
            .steps
            .iter()
            .map(|step| match step.hz {
                Some(hz) => format!("{hz} Hz at {:.0} ms", step.at_ms),
                None => format!("Baseline tier at {:.0} ms", step.at_ms),
            })
            .collect();
        format!(
            "asked for {} Hz, then {}; {} Hz at the end",
            run.display_hz,
            steps.join(", "),
            self.hz
        )
    }

    /// The summary as `just b3d frames` prints it, a few lines.
    #[must_use]
    pub fn print(&self, run: &Run) -> String {
        let mut lines = vec![
            format!(
                "{} frames in {:.1} s, each on a tick of the {} Hz display ({} window, {})",
                self.frames, self.seconds, run.display_hz, run.window, run.adapter
            ),
            self.print_rate(run),
            format!(
                "late: {} ({} held to 60 Hz, {} missed 60 Hz); worst gap {:.1} ms",
                self.late, self.held_to_60, self.missed_60, self.worst_gap_ms
            ),
            format!(
                "waited for a drawable: median {:.1} ms, 95th percentile {:.1} ms, worst {:.1} ms; \
                 {:.1} ms in all; {} frames found none",
                self.median_wait_ms,
                self.p95_wait_ms,
                self.worst_wait_ms,
                self.total_wait_ms,
                self.skipped
            ),
        ];
        if let Some(conditions) = run.conditions() {
            lines.insert(1, conditions);
        }
        let searching = &self.searching;
        if searching.frames > 0 {
            lines.push(format!(
                "while the AI searched: {} frames, {} late; worst client {:.2} ms, worst encode \
                 {:.2} ms",
                searching.frames,
                searching.late,
                searching.worst_client_ms,
                searching.worst_encoded_ms
            ));
        }
        if let Some(pause) = run.pause_at_ms {
            lines.push(format!(
                "paused at {pause} ms: {} frames drawn after the pause",
                self.after_pause
            ));
        }
        if self.faceless_cards > 0 || self.missing_glyphs > 0 {
            lines.push(format!(
                "in the last frame: {} cards drawn blank with no face, {} glyphs missing",
                self.faceless_cards, self.missing_glyphs
            ));
        }
        if !self.unbuilt.is_empty() {
            lines.push(format!(
                "played as instant changes, not built yet: {}",
                self.unbuilt.join(", ")
            ));
        }
        if !self.sounds.is_empty() {
            let played: Vec<String> = (self.sounds.iter())
                .map(|(name, count)| format!("{name} {count}"))
                .collect();
            lines.push(format!("sounds played: {}", played.join(", ")));
        }
        if self.unplaced > 0 {
            lines.push(format!(
                "script steps the table could not take: {}",
                self.unplaced
            ));
        }
        lines.join("\n")
    }
}

/// Nanoseconds as milliseconds.
fn ms(nanoseconds: u64) -> f64 {
    nanoseconds as f64 / 1e6
}
