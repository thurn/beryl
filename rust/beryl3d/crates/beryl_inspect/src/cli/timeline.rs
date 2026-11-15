//! `timeline` and `export`: when each step of each script of a recording begins and ends, as
//! JSON lines, and the recordings' messages as the fixture files the web interface reads
//! (docs/beryl-3d.md, "Timeline parity").

use std::fmt::Write;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use beryl_protocol::protocol::Side;
use beryl_protocol::script::Edge;
use beryl_scene::driver::TimelineStep;
use serde_json::Value;

use super::options::{self, Options, Usage, seat_word, side_of};
use super::{Printed, open};
use crate::{pack, pages, recordings};

/// The switch that prints every recording and seat to a file.
pub(super) const ALL: &str = "--all";

/// A number of milliseconds, from nanoseconds, to 3 decimal places.
fn ms(nanoseconds: u64) -> String {
    format!("{:.3}", nanoseconds as f64 / 1_000_000.0)
}

/// The text `s` as a JSON string.
fn string(s: &str) -> String {
    Value::String(s.to_owned()).to_string()
}

/// One step as a line of JSON whose keys are in a fixed order.
fn line(name: &str, seat: Side, step: &TimelineStep) -> String {
    let case = step
        .case
        .as_deref()
        .map_or_else(|| "null".to_owned(), string);
    let effects: Vec<String> = step
        .effects
        .iter()
        .map(|(effect, edge, into)| {
            let edge = match edge {
                Edge::Start => "start",
                Edge::End => "end",
            };
            let into = into.map_or_else(String::new, |into| format!("+{into:.3}"));
            string(&format!("{effect}@{edge}{into}"))
        })
        .collect();
    format!(
        "{{\"recording\":{},\"seat\":{},\"view\":{},\"beat\":{},\"step\":{},\"verb\":{},\
         \"case\":{case},\"start\":{},\"end\":{},\"effects\":[{}]}}",
        string(name),
        string(seat_word(seat)),
        step.view,
        step.beat,
        step.index,
        string(step.verb.name()),
        ms(step.from),
        ms(step.to),
        effects.join(",")
    )
}

/// The lines of the recording `name` as `seat` sees it.
fn lines(name: &str, seat: Side, layout: beryl_scene::agent::Layout) -> Result<Vec<String>, Usage> {
    let size = options_window(layout);
    let mut page = pages::page(name, seat, layout, size).map_err(Usage)?;
    Ok(page
        .timeline()
        .iter()
        .map(|step| line(name, seat, step))
        .collect())
}

fn options_window(layout: beryl_scene::agent::Layout) -> beryl_scene::input::Size {
    match layout {
        beryl_scene::agent::Layout::Desktop => beryl_scene::input::Size {
            width: 1600.0,
            height: 900.0,
        },
        beryl_scene::agent::Layout::Phone => beryl_scene::input::Size {
            width: 393.0,
            height: 759.0,
        },
    }
}

pub(super) fn timeline(options: &Options) -> Result<Printed, Usage> {
    if options.switch(ALL) {
        return all(options);
    }
    // Opens the page to check the name and the seat, with the same messages as `info` gives.
    open(options)?;
    let mut out = format!("{}\n", options.command_line());
    for text in lines(options.name(), options.seat, options.layout)? {
        let _ = writeln!(out, "{text}");
    }
    Ok(Printed::text(out))
}

/// Writes the lines of every recording and seat to a file under the cache's `out` folder.
fn all(options: &Options) -> Result<Printed, Usage> {
    let names = recordings::names().map_err(Usage)?;
    let mut text = String::new();
    let mut recordings = 0;
    let mut count = 0;
    let mut previous: Option<&str> = None;
    for (name, seat) in &names {
        let seat = side_of(*seat);
        for line in lines(name, seat, options.layout)? {
            text.push_str(&line);
            text.push('\n');
            count += 1;
        }
        if previous != Some(name.as_str()) {
            recordings += 1;
        }
        previous = Some(name);
    }
    let directory = pack::cache_dir().join("out");
    fs::create_dir_all(&directory).map_err(|e| Usage(format!("{}: {e}", directory.display())))?;
    let run = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let path = directory.join(format!("timeline-{run}.jsonl"));
    fs::write(&path, text).map_err(|e| Usage(format!("{}: {e}", path.display())))?;
    Ok(Printed::text(format!(
        "{}\n{}\n{recordings} recordings, {count} lines\n",
        options.command_line(),
        path.display()
    )))
}

pub(super) fn export(options: &Options) -> Result<Printed, Usage> {
    let directory = match options.rest.as_slice() {
        [directory] => PathBuf::from(directory),
        _ => {
            return Err(Usage(
                "export takes one argument, the directory to write to, as in \
                 `just b3d export /tmp/recordings`"
                    .to_owned(),
            ));
        }
    };
    let names = recordings::names().map_err(Usage)?;
    fs::create_dir_all(&directory).map_err(|e| Usage(format!("{}: {e}", directory.display())))?;
    let mut files = 0;
    for (name, seat) in &names {
        let seat = side_of(*seat);
        let recording = pages::recording(name, seat).map_err(Usage)?;
        let json =
            serde_json::to_string(&recording.0).map_err(|e| Usage(format!("{name}: {e}")))?;
        let file = directory.join(format!(
            "{}.{}.json",
            name.replace('/', "__"),
            options::seat_word(seat)
        ));
        fs::write(&file, json).map_err(|e| Usage(format!("{}: {e}", file.display())))?;
        files += 1;
    }
    Ok(Printed::text(format!(
        "{}\n{}\n{files} files\n",
        options.command_line(),
        directory.display()
    )))
}
