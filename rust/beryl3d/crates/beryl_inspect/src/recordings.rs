//! Every recording Beryl 3D's tests and tools read: each fixture in `ui/fixtures.json` and each
//! card spec moment, as the session's messages to one seat. This is the only way into the
//! recorder, so the engine runs in one place: on the first call in a process, when the pack for
//! the present sources is missing (see [`crate::pack`]).
//!
//! The pack is read into memory whole on that first call, and each recording's messages are
//! parsed once, on the first request for them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use beryl_scene::agent::Layout;
use beryl_scene::driver::{Page, Recording};
use beryl_scene::input::Size;
use beryl_session::Seat;
use beryl_session::ipc::FixtureMessage;
use beryl_session::protocol::Side;

use crate::pack::{self, Entry};
use crate::recorder::Cli;

/// One recording, as `just b3d list` shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    /// A fixture's name, or a card spec moment's as `<card>/<moment>`.
    pub name: String,
    /// The seat its messages are sent to.
    pub seat: Seat,
    /// How long it plays on its own clock with no action taken, in milliseconds: until the
    /// table rests after its last view. None if it could not be recorded, or played.
    pub length_ms: Option<u64>,
    /// How many views it holds.
    pub views: u32,
    /// Why it could not be recorded, if it could not.
    pub failure: Option<String>,
}

/// The messages of the recording `name` as `seat` is sent them.
///
/// # Panics
///
/// When the pack cannot be built or read, when no recording is named `name` for `seat`, or
/// when it could not be recorded: each says why.
#[must_use]
pub fn messages(name: &str, seat: Seat) -> Arc<[FixtureMessage]> {
    find(name, seat).unwrap_or_else(|why| panic!("{why}"))
}

/// [`messages`], or why it cannot be had.
///
/// # Errors
///
/// When the pack cannot be built or read, when no recording is named `name` for `seat`, or
/// when it could not be recorded.
pub fn find(name: &str, seat: Seat) -> Result<Arc<[FixtureMessage]>, String> {
    let opened = opened()?;
    let at = opened
        .entries
        .binary_search_by(|e| (e.name, e.seat).cmp(&(name, seat.get())))
        .map_err(|_| missing(&opened.entries, name, seat))?;
    let mut parsed = opened
        .parsed
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(messages) = parsed.get(&at) {
        return Ok(Arc::clone(messages));
    }
    let messages: Arc<[FixtureMessage]> = pack::messages(&opened.entries[at])?.into();
    parsed.insert(at, Arc::clone(&messages));
    Ok(messages)
}

/// Every recording, sorted by name and seat, each with its length and number of views. Finding
/// a length plays the recording through the client, on as many threads as the machine has
/// cores.
///
/// # Errors
///
/// When the pack cannot be built or read.
pub fn list() -> Result<Vec<Listed>, String> {
    let opened = opened()?;
    let next = std::sync::atomic::AtomicUsize::new(0);
    let lengths = Mutex::new(vec![None; opened.entries.len()]);
    let threads = std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .min(opened.entries.len().max(1));
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let at = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(entry) = opened.entries.get(at) else {
                        break;
                    };
                    let length = length(entry);
                    lengths
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)[at] = length;
                }
            });
        }
    });
    let lengths = lengths
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(opened
        .entries
        .iter()
        .zip(lengths)
        .map(|(entry, length_ms)| Listed {
            name: entry.name.to_owned(),
            seat: entry.seat(),
            length_ms,
            views: entry.views,
            failure: entry.messages.err().map(str::to_owned),
        })
        .collect())
}

/// How long `entry` plays with no action taken, in milliseconds, on the desktop layout.
fn length(entry: &Entry) -> Option<u64> {
    let messages = pack::messages(entry).ok()?;
    let recording = Recording(messages);
    // A recording the client cannot play has no length; its tests say why.
    std::panic::catch_unwind(|| {
        let size = Size {
            width: 1440.0,
            height: 900.0,
        };
        let mut page = Page::new(&recording, Side::You, Layout::Desktop, size).ok()?;
        page.moments().last().map(|(_, at)| at.0 / 1_000_000)
    })
    .ok()
    .flatten()
}

/// The pack for the present sources, built first if it is missing, as read on the first call.
struct Opened {
    entries: Vec<Entry<'static>>,
    /// Each recording's messages once parsed, by its index in `entries`.
    parsed: Mutex<BTreeMap<usize, Arc<[FixtureMessage]>>>,
}

fn opened() -> Result<&'static Opened, String> {
    static OPENED: OnceLock<Result<Opened, String>> = OnceLock::new();
    OPENED.get_or_init(open).as_ref().map_err(Clone::clone)
}

fn open() -> Result<Opened, String> {
    let root = root();
    let files = pack::sources::sources(&root)?;
    let hash = pack::sources::hash(&root, &files)?;
    let path = pack::ensure(&pack::cache_dir(), &hash, &Cli { root })?;
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    // Kept for the life of the process, so that entries can borrow their names and messages.
    let bytes: &'static [u8] = Vec::leak(bytes);
    let entries = pack::read(bytes).map_err(|why| format!("{}: {why}", path.display()))?;
    Ok(Opened {
        entries,
        parsed: Mutex::new(BTreeMap::new()),
    })
}

/// The repository this crate was built from.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../..")
}

/// Why no recording is named `name` for `seat`: the seats it has, or the five nearest names.
fn missing(entries: &[Entry], name: &str, seat: Seat) -> String {
    let seats: Vec<&str> = entries
        .iter()
        .filter(|e| e.name == name)
        .map(|e| if e.seat == 0 { "P0" } else { "P1" })
        .collect();
    if !seats.is_empty() {
        return format!(
            "the recording {name:?} is not recorded for {seat:?}, only for {}",
            seats.join(" and ")
        );
    }
    let mut names: Vec<&str> = entries.iter().map(|e| e.name).collect();
    names.dedup();
    names.sort_by_key(|n| distance(n, name));
    names.truncate(5);
    format!(
        "no recording is named {name:?}; the nearest are {}",
        names.join(", ")
    )
}

/// The edit distance between `a` and `b`, by characters.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (diagonal + usize::from(ca != cb))
                .min(row[j] + 1)
                .min(above + 1);
            diagonal = above;
        }
    }
    row[b.len()]
}
