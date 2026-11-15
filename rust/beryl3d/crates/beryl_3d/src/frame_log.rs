//! The frame log (docs/beryl-3d.md, "Driving the running app"): every run of the app writes a
//! file of its own, `$BERYL_CACHE/b3d/frames/<start time>-<process id>.jsonl`, that an agent
//! reads with `just b3d frames`. Its first line describes the run, then one line follows for
//! each late frame, and the last line is the run's summary.
//!
//! A frame is late when more than one and a half display intervals passed since the frame
//! before it while the ticks ran. On a display faster than 60 Hz, a late frame that still came
//! within one and a half sixtieths of a second was held to 60 Hz, which is counted apart from
//! one that missed 60 Hz.
//!
//! The frame thread does no file I/O (docs/beryl-3d.md, "Threads"): it counts frames itself
//! and sends each line to a thread of the log's own, which writes it.

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};
use std::thread::JoinHandle;
use std::time::Instant;

use beryl_host::Ticked;
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
    /// Whether it still came within one and a half sixtieths of a second, on a faster display.
    pub held_to_60: bool,
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
    /// Late frames that came within one and a half sixtieths of a second, on a faster display.
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
}

/// One line of the log.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Line {
    Run(Run),
    Late(Late),
    Summary(Summary),
}

/// A log as `read` found it.
#[derive(Clone, Debug, PartialEq)]
pub struct Log {
    pub run: Run,
    pub late: Vec<Late>,
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
    display_interval: f64,
    faster_than_60: bool,
    pause_at: Option<u64>,
    summary: Summary,
    /// Whether a frame has been drawn at or after the pause.
    reached_pause: bool,
    /// How many shown frames waited for a drawable for each tenth of a millisecond, made once
    /// so that counting a frame allocates nothing.
    waits: Box<[u32; WAIT_BUCKETS]>,
}

impl FrameLog {
    /// Starts a log for `run` in `dir`, named for its start time and process.
    ///
    /// # Errors
    ///
    /// When the file cannot be created or written.
    pub fn create(dir: &Path, run: &Run) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        let path = dir.join(format!("{}-{}.jsonl", run.started_ms, run.pid));
        let mut file = BufWriter::new(File::create(&path)?);
        write(&mut file, &Line::Run(run.clone()))?;
        file.flush()?;
        let (lines, received) = channel::<Line>();
        let writer = std::thread::Builder::new()
            .name("frame log".to_owned())
            .spawn(move || {
                beryl_host::utility_thread();
                for line in received {
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
            display_interval: 1e9 / f64::from(run.display_hz.max(1.0)),
            faster_than_60: run.display_hz > 61.0,
            pause_at: run.pause_at_ms.map(|ms| (ms * 1e6) as u64),
            summary: Summary::default(),
            reached_pause: false,
            waits: Box::new([0; WAIT_BUCKETS]),
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Counts a frame the host drew, and sends a line for it to be written when it was late.
    pub fn frame(&mut self, ticked: &Ticked, verbs: &[&str], searching: bool) {
        let summary = &mut self.summary;
        summary.frames += 1;
        if let Some(pause) = self.pause_at
            && ticked.tick >= pause
        {
            if self.reached_pause {
                summary.after_pause += 1;
            }
            self.reached_pause = true;
        }
        let Some(shown) = ticked.shown else {
            summary.skipped += 1;
            return;
        };
        summary.first_shown_ms.get_or_insert(ms(ticked.tick));
        let waited = ms(shown.waited);
        let bucket = usize::try_from(shown.waited / WAIT_BUCKET).unwrap_or(usize::MAX);
        self.waits[bucket.min(WAIT_BUCKETS - 1)] += 1;
        summary.worst_wait_ms = summary.worst_wait_ms.max(waited);
        summary.total_wait_ms += waited;
        let Some(gap) = ticked.since_last else {
            return;
        };
        summary.worst_gap_ms = summary.worst_gap_ms.max(ms(gap));
        if (gap as f64) <= LATE * self.display_interval {
            return;
        }
        let held_to_60 = self.faster_than_60 && (gap as f64) <= LATE * SIXTIETH;
        summary.late += 1;
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
        };
        // The writer ends only with the log, so a failed send has nowhere to go.
        let _ = self.lines.send(Line::Late(late));
    }

    /// Notes a verb the client played as an instant change.
    pub fn unbuilt(&mut self, verb: &str) {
        self.summary.unbuilt.push(verb.to_owned());
    }

    /// Writes the summary as the last line and returns it.
    ///
    /// # Errors
    ///
    /// When a line could not be written.
    pub fn finish(mut self) -> io::Result<Summary> {
        self.summary.seconds = self.created.elapsed().as_secs_f64();
        let worst = self.summary.worst_wait_ms;
        self.summary.median_wait_ms = percentile(&self.waits[..], 0.5).min(worst);
        self.summary.p95_wait_ms = percentile(&self.waits[..], 0.95).min(worst);
        let summary = self.summary;
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
        summary: None,
    };
    for line in lines {
        match line? {
            Line::Run(_) => return Err(format!("{}: a second run line", path.display())),
            Line::Late(late) => log.late.push(late),
            Line::Summary(summary) => log.summary = Some(summary),
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
        (!parts.is_empty()).then(|| parts.join(", "))
    }
}

impl Summary {
    /// The summary as `just b3d frames` prints it, a few lines.
    #[must_use]
    pub fn print(&self, run: &Run) -> String {
        let mut lines = vec![
            format!(
                "{} frames in {:.1} s, each on a tick of the {} Hz display ({} window, {})",
                self.frames, self.seconds, run.display_hz, run.window, run.adapter
            ),
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
        if let Some(pause) = run.pause_at_ms {
            lines.push(format!(
                "paused at {pause} ms: {} frames drawn after the pause",
                self.after_pause
            ));
        }
        if !self.unbuilt.is_empty() {
            lines.push(format!(
                "played as instant changes, not built yet: {}",
                self.unbuilt.join(", ")
            ));
        }
        lines.join("\n")
    }
}

/// Nanoseconds as milliseconds.
fn ms(nanoseconds: u64) -> f64 {
    nanoseconds as f64 / 1e6
}
