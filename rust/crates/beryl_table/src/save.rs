//! The host's append-only manual-table save format.

use std::fmt;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom, Write};

use serde::{Deserialize, Serialize};

use crate::state::MarkerSource;
use crate::{Chance, Entry, Log, Refusal, Table};

const FORMAT: u32 = 1;

#[derive(Debug)]
pub enum SaveError {
    Io(io::Error),
    Json(serde_json::Error),
    MissingHeader,
    UnsupportedFormat(u32),
    Replay { entry: u32, refusal: Refusal },
    InvalidEntry(u32),
}

impl fmt::Display for SaveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Json(error) => error.fmt(formatter),
            Self::MissingHeader => formatter.write_str("Manual table save is missing its header"),
            Self::UnsupportedFormat(format) => {
                write!(formatter, "Unsupported manual table save format {format}")
            }
            Self::Replay { entry, refusal } => write!(
                formatter,
                "Could not replay table entry {entry}: {refusal:?}"
            ),
            Self::InvalidEntry(entry) => {
                write!(formatter, "Invalid or inconsistent table entry {entry}")
            }
        }
    }
}

impl std::error::Error for SaveError {}

impl From<io::Error> for SaveError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for SaveError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[derive(Deserialize, Serialize)]
struct Header<T> {
    format: u32,
    seed: u64,
    initial: T,
}

/// Writes the first line. Use the table from before the first saved entry.
pub fn write_header(writer: impl Write, initial: &Table, seed: u64) -> Result<(), SaveError> {
    write_line(
        writer,
        &Header {
            format: FORMAT,
            seed,
            initial,
        },
    )
}

/// The file stores the action and its outcomes. Derived changes, snapshots
/// and log wording can evolve without changing the save format.
#[derive(Serialize, Deserialize)]
struct SavedEntry<T> {
    n: u32,
    by: crate::Seat,
    action: T,
    outcomes: Vec<u32>,
}

/// Appends one complete entry to the host's save.
pub fn append(writer: impl Write, entry: &Entry) -> Result<(), SaveError> {
    write_line(
        writer,
        &SavedEntry {
            n: entry.n,
            by: entry.by,
            action: &entry.action,
            outcomes: crate::log::outcomes(&entry.changes).collect(),
        },
    )
}

fn write_line(mut writer: impl Write, data: &impl Serialize) -> Result<(), SaveError> {
    let mut line = serde_json::to_vec(data)?;
    line.push(b'\n');
    writer.write_all(&line)?;
    Ok(())
}

// Read the version independently so a newer header can change its shape.
#[derive(Deserialize)]
struct Version {
    format: u32,
}

/// Rebuilds the table and its snapshots from the recorded actions and outcomes.
pub fn open(reader: impl BufRead) -> Result<(Table, Log), SaveError> {
    let (table, log, _) = open_with_chance(reader)?;
    Ok((table, log))
}

/// Also restores the host's random stream so play can resume after the save.
pub fn open_with_chance(reader: impl BufRead) -> Result<(Table, Log, Chance), SaveError> {
    let (state, _) = read_save(reader)?;
    Ok(state)
}

/// Reopens an existing read/write save file. Discards an interrupted final
/// line and positions the file for the next append.
pub fn open_for_append(file: &mut File) -> Result<(Table, Log, Chance), SaveError> {
    file.rewind()?;
    let (state, complete_len) = read_save(BufReader::new(&mut *file))?;
    file.set_len(complete_len)?;
    file.seek(SeekFrom::Start(complete_len))?;
    Ok(state)
}

fn read_save(mut reader: impl BufRead) -> Result<((Table, Log, Chance), u64), SaveError> {
    let mut line = Vec::new();
    let mut complete_len = reader.read_until(b'\n', &mut line)? as u64;
    if !line.ends_with(b"\n") {
        return Err(SaveError::MissingHeader);
    }
    let version: Version = serde_json::from_slice(&line)?;
    if version.format != FORMAT {
        return Err(SaveError::UnsupportedFormat(version.format));
    }
    let header: Header<Table> = serde_json::from_slice(&line)?;
    let mut table = header.initial;
    // A format-1 header written before markers captured their source may still
    // have its source object. Entries below rebuild snapshots during replay.
    for marker in table.markers.values_mut() {
        if marker.display_source.is_none() {
            marker.display_source = table.objects.get(&marker.source).map(MarkerSource::capture);
        }
    }
    let mut log = Log::new(&table);
    let mut fresh = Chance::fresh(header.seed);
    loop {
        line.clear();
        let bytes = reader.read_until(b'\n', &mut line)?;
        if !line.ends_with(b"\n") {
            // A crash can interrupt the last append. The prior complete lines
            // are still valid and contain all previously committed actions.
            break;
        }
        let saved: SavedEntry<crate::Action> = serde_json::from_slice(&line)?;
        if log.tip().checked_add(1) != Some(saved.n) {
            return Err(SaveError::InvalidEntry(saved.n));
        }
        let mut replay = fresh.replay_and_advance(saved.outcomes);
        log.apply(&mut table, saved.by, &saved.action, &mut replay)
            .map_err(|refusal| SaveError::Replay {
                entry: saved.n,
                refusal,
            })?;
        if !replay.update_fresh(&mut fresh) {
            return Err(SaveError::InvalidEntry(saved.n));
        }
        complete_len += bytes as u64;
    }
    Ok(((table, log, fresh), complete_len))
}
