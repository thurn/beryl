//! Reads frame logs from completed and in-progress runs.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use super::{Line, Log, Run, Summary};

/// Reads the log at `path`.
///
/// # Errors
///
/// When the file cannot be read, a line is not a log line, or the first line does not
/// describe a run.
pub fn read(path: &Path) -> Result<Log, String> {
    let mut reader = open(path)?;
    let run = header(&mut reader, path)?;
    let mut text = String::new();
    let mut log = Log {
        run,
        late: Vec::new(),
        inputs: Vec::new(),
        summary: None,
        tier: None,
        timing_windows: Vec::new(),
        pacing_changes: Vec::new(),
    };
    for index in 2.. {
        text.clear();
        if reader
            .read_line(&mut text)
            .map_err(|error| io_error(path, &error))?
            == 0
        {
            break;
        }
        let line = match serde_json::from_str::<Line>(&text) {
            Ok(line) => line,
            // An in-progress or interrupted flush can end partway through the last line.
            Err(cause) if cause.is_eof() && !text.ends_with('\n') => break,
            Err(cause) => return Err(format!("{}:{index}: {cause}", path.display())),
        };
        match line {
            Line::Run(_) => return Err(format!("{}: a second run line", path.display())),
            Line::FirstShownMs(_) | Line::LifecycleShownMs(_) => {}
            Line::TimingWindow(window) => log.timing_windows.push(window),
            Line::PacingChange(change) => log.pacing_changes.push(change),
            Line::Late(late) => log.late.push(late),
            Line::Summary(summary) => log.summary = Some(summary),
            Line::Input(input) => log.inputs.push(input.input),
            Line::Tier(tiered) => log.tier = Some(tiered),
        }
    }
    Ok(log)
}

pub(crate) fn read_run(path: &Path) -> Result<Run, String> {
    header(&mut open(path)?, path)
}

fn open(path: &Path) -> Result<BufReader<File>, String> {
    File::open(path)
        .map(BufReader::new)
        .map_err(|error| io_error(path, &error))
}

fn io_error(path: &Path, error: &std::io::Error) -> String {
    format!("{}: {error}", path.display())
}

fn header(reader: &mut BufReader<File>, path: &Path) -> Result<Run, String> {
    let mut text = String::new();
    reader
        .read_line(&mut text)
        .map_err(|error| io_error(path, &error))?;
    match serde_json::from_str::<Line>(&text) {
        Ok(Line::Run(run)) => Ok(run),
        _ => Err(format!(
            "{}: the first line does not describe a run",
            path.display()
        )),
    }
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

    /// The summary as `just client frames` prints it, a few lines.
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
