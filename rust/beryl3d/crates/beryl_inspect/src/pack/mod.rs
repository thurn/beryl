//! The recordings pack: every recording in one file, built once for each state of the sources
//! a recording depends on and shared by every worktree on the machine (docs/beryl-3d.md,
//! "Recordings").
//!
//! The file is `recordings-<hash>.bin` in the cache directory, where the hash is
//! [`sources::hash`] of [`sources::sources`]. Of several processes that find it missing, one
//! builds it while holding `recordings-<hash>.lock`, writes it under a name of its own and
//! renames it into place; the others wait on the lock and read the finished file. A pack that
//! was built or read in the last [`KEPT_FOR`] is kept, as are the newest [`KEPT`], up to
//! [`MOST`]; older ones are deleted.
//!
//! The file is postcard: the format version, then each recording's name, seat, number of views
//! and either its messages as deflated JSON text or why it could not be recorded. A message
//! holds JSON values, which postcard cannot read back, so each recording's messages stay JSON,
//! deflated one recording at a time (a pack's views repeat, so about 30 times smaller), and are
//! parsed only when that recording is asked for.

pub mod sources;

use std::fs::{self, File};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use beryl_protocol::Seat;
use beryl_protocol::ipc::{FixtureMessage, GAME_UPDATE};
use serde::{Deserialize, Serialize};

/// How many packs the cache keeps however old they are: the newest, by when they were last
/// built or read.
pub const KEPT: usize = 3;

/// How long after its last build or read a pack is kept. When the cache kept only the newest
/// [`KEPT`], the worktrees of the machine, each at its own state of the sources, built three
/// packs within three minutes on 2026-11-16, so a fast check's pack was deleted while its
/// tests still ran and they failed with "the recordings pack for the present sources is
/// missing".
pub const KEPT_FOR: Duration = Duration::from_hours(6);

/// The most packs kept, whatever their age: a pack is about 15 MB.
pub const MOST: usize = 48;

/// A pack read again sooner than this after its last mark is not marked again (`mark_used`).
const MARK_EVERY: Duration = Duration::from_secs(60);

/// The pack format's version, which a reader requires. The hash covers it too, so a pack of an
/// older format is built again, not misread.
pub const VERSION: u32 = 2;

/// How hard each recording is deflated: of the levels measured, 6 made the pack 8 MB from
/// 261 MB of JSON and 1 made it 16 MB, at a cost of about a second of CPU time over the pack.
const DEFLATE_LEVEL: u8 = 6;

/// One recording, as a recorder made it: its name (a fixture's, or a card spec moment's as
/// `<card>/<moment>`), the seat it is seen from, and its messages or why it could not be made.
#[derive(Clone, Debug, PartialEq)]
pub struct Recorded {
    pub name: String,
    pub seat: Seat,
    pub messages: Result<Vec<FixtureMessage>, String>,
}

/// What makes recordings: `beryl_cli`'s fixture and spec recorders, or a test's stand-in.
pub trait Recorder: Sync {
    /// Every recording, in any order.
    fn record(&self) -> Vec<Recorded>;
}

/// One recording as the pack holds it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry<'a> {
    pub name: &'a str,
    /// The seat's index: 0 or 1.
    pub seat: u8,
    /// How many `game-update` events its messages hold.
    pub views: u32,
    /// Its messages as a JSON array, deflated, or why it could not be recorded. Postcard
    /// writes a slice of bytes as its length and then the bytes, and reads it back borrowed.
    #[serde(borrow)]
    pub messages: Result<&'a [u8], &'a str>,
}

impl Entry<'_> {
    #[must_use]
    pub fn seat(&self) -> Seat {
        Seat::try_new(self.seat).unwrap_or(Seat::P1)
    }
}

#[derive(Serialize, Deserialize)]
struct Contents<'a> {
    version: u32,
    #[serde(borrow)]
    entries: Vec<Entry<'a>>,
}

/// The path of the pack built from sources whose hash is `hash`, in `dir`.
#[must_use]
pub fn path(dir: &Path, hash: &str) -> PathBuf {
    dir.join(format!("recordings-{hash}.bin"))
}

/// Beryl 3D's part of the per-machine cache: `$BERYL_CACHE/b3d`, where `$BERYL_CACHE` is
/// `~/Library/Caches/beryl` unless set, or `%LOCALAPPDATA%\beryl` on Windows.
#[must_use]
pub fn cache_dir() -> PathBuf {
    let cache = match std::env::var_os("BERYL_CACHE") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ if cfg!(windows) => {
            PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("beryl")
        }
        _ => {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Library/Caches/beryl")
        }
    };
    cache.join("b3d")
}

/// The path of the pack for `hash` in `dir`, built first with `recorder` if it is missing. A
/// build prints one line saying so and how long it took, so that it is reported apart from the
/// time of whatever asked for it.
///
/// # Errors
///
/// When the directory, the lock or the file cannot be written.
pub fn ensure(dir: &Path, hash: &str, recorder: &dyn Recorder) -> Result<PathBuf, String> {
    let pack = path(dir, hash);
    if pack.is_file() {
        mark_used(&pack);
        return Ok(pack);
    }
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let lock_path = dir.join(format!("recordings-{hash}.lock"));
    let lock = File::create(&lock_path).map_err(|e| format!("{}: {e}", lock_path.display()))?;
    // Released when `lock` is dropped, or by the system if this process dies holding it.
    lock.lock()
        .map_err(|e| format!("{}: {e}", lock_path.display()))?;
    if pack.is_file() {
        // Another process built it while this one waited.
        return Ok(pack);
    }
    let started = Instant::now();
    let bytes = encode(recorder.record());
    let temporary = dir.join(format!("recordings-{hash}.bin.{}.tmp", std::process::id()));
    let written = File::create(&temporary)
        .and_then(|mut file| {
            file.write_all(&bytes)?;
            file.sync_all()
        })
        .and_then(|()| fs::rename(&temporary, &pack));
    if let Err(e) = written {
        let _ = fs::remove_file(&temporary);
        return Err(format!("{}: {e}", pack.display()));
    }
    eprintln!(
        "recordings: built {} ({} KB) in {:.1} s",
        pack.display(),
        bytes.len() / 1024,
        started.elapsed().as_secs_f64()
    );
    prune(dir, &pack);
    drop(lock);
    Ok(pack)
}

/// The pack's bytes for `recorded`, sorted by name and seat.
fn encode(mut recorded: Vec<Recorded>) -> Vec<u8> {
    recorded.sort_by(|a, b| (&a.name, a.seat).cmp(&(&b.name, b.seat)));
    // Each recording's messages as deflated JSON, if it was made.
    let deflated: Vec<Option<Vec<u8>>> = recorded
        .iter()
        .map(|r| {
            let json = serde_json::to_vec(r.messages.as_ref().ok()?).expect("messages serialize");
            Some(miniz_oxide::deflate::compress_to_vec(&json, DEFLATE_LEVEL))
        })
        .collect();
    let file = Contents {
        version: VERSION,
        entries: recorded
            .iter()
            .zip(&deflated)
            .map(|(r, deflated)| Entry {
                name: &r.name,
                seat: r.seat.get(),
                views: r.messages.as_ref().map_or(0, |messages| views(messages)),
                messages: match (&r.messages, deflated) {
                    (Ok(_), Some(bytes)) => Ok(bytes),
                    (Err(why), _) => Err(why),
                    (Ok(_), None) => unreachable!("a made recording is deflated"),
                },
            })
            .collect(),
    };
    postcard::to_stdvec(&file).expect("a pack serializes")
}

/// How many `game-update` events `messages` holds.
fn views(messages: &[FixtureMessage]) -> u32 {
    let views = messages
        .iter()
        .filter(|m| matches!(m, FixtureMessage::Event { event, .. } if event == GAME_UPDATE))
        .count();
    u32::try_from(views).unwrap_or(u32::MAX)
}

/// The recordings in a pack's bytes, sorted by name and seat.
///
/// # Errors
///
/// When the bytes are not a pack of this version.
pub fn read(bytes: &[u8]) -> Result<Vec<Entry<'_>>, String> {
    let file: Contents = postcard::from_bytes(bytes).map_err(|e| format!("not a pack: {e}"))?;
    if file.version != VERSION {
        return Err(format!("a pack of version {}, not {VERSION}", file.version));
    }
    Ok(file.entries)
}

/// The messages of one recording in a pack.
///
/// # Errors
///
/// When it could not be recorded, or its bytes are not deflated messages.
pub fn messages(entry: &Entry) -> Result<Vec<FixtureMessage>, String> {
    let deflated = entry.messages.map_err(str::to_owned)?;
    let json = miniz_oxide::inflate::decompress_to_vec(deflated)
        .map_err(|e| format!("{}: not deflated: {e}", entry.name))?;
    serde_json::from_slice(&json).map_err(|e| format!("{}: not messages: {e}", entry.name))
}

/// Sets the modification time of the pack at `path` to now, which [`prune`] reads as its last
/// use, unless it was set within [`MARK_EVERY`]. Nothing that fails here matters to the caller.
pub fn mark_used(path: &Path) {
    let stale = fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_none_or(|age| age > MARK_EVERY);
    if stale && let Ok(file) = File::options().write(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

/// Deletes the packs in `dir` that are neither among the newest [`KEPT`] nor used within
/// [`KEPT_FOR`], and all but the newest [`MOST`], never `keep`, with their lock files and any
/// temporary file left by a build that died. Nothing that fails here matters to the caller.
fn prune(dir: &Path, keep: &Path) {
    let Ok(listing) = fs::read_dir(dir) else {
        return;
    };
    let mut packs = Vec::new();
    for entry in listing.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("recordings-") {
            continue;
        }
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default();
        if extension == "bin" {
            let written = entry.metadata().and_then(|m| m.modified()).ok();
            packs.push((path == keep, written, path));
        } else if extension == "tmp" && !name.contains(&format!(".{}.", std::process::id())) {
            // A build in another process may still be writing it: only one more than a day
            // old is a dead build's.
            let old = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age.as_secs() > 86_400);
            if old {
                let _ = fs::remove_file(path);
            }
        }
    }
    // The pack just built first, then the newest.
    packs.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    for (at, (_, written, path)) in packs.into_iter().enumerate() {
        let recent = written
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age <= KEPT_FOR);
        if at < KEPT || (at < MOST && recent) {
            continue;
        }
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(path.with_extension("lock"));
    }
}
