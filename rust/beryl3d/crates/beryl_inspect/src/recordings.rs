//! Every recording Beryl 3D's tests and tools read: each fixture in `ui/fixtures.json` and each
//! card spec moment, as the session's messages to one seat, read from the pack for the present
//! sources (see [`crate::pack`]). This crate does not play the engine: the `record` program
//! (`beryl_record`) builds a missing pack with [`build`], and a read of a missing one fails
//! with a message that starts with [`MISSING`].
//!
//! The pack is read into memory whole on that first call, and each recording's messages are
//! parsed once, on the first request for them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use app_client::driver::{Page, Recording};
use beryl_protocol::Seat;
use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::protocol::Side;
use kit_probe::agent::Layout;
use kit_ui::input::Size;

use crate::pack::{self, Entry, Recorder};
use crate::stress::Board;
use crate::{pages, screens, stories};

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
    let parsed = || {
        opened
            .parsed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    };
    if let Some(messages) = parsed().get(&at) {
        return Ok(Arc::clone(messages));
    }
    // Parsed with the lock released, so that the interface tests' threads parse different
    // recordings at once. Two threads may parse the same one; the first to finish keeps it.
    let messages: Arc<[FixtureMessage]> = pack::messages(&opened.entries[at])?.into();
    Ok(Arc::clone(parsed().entry(at).or_insert(messages)))
}

/// The pack's entries in name and seat order, without decoding their messages.
pub(crate) fn entries() -> Result<&'static [Entry<'static>], String> {
    Ok(&opened()?.entries)
}

/// The name and seat of every recording, stress boards included, sorted by name and seat,
/// without playing any.
///
/// # Errors
///
/// When the pack cannot be built or read.
pub fn names() -> Result<Vec<(String, Seat)>, String> {
    let opened = opened()?;
    let mut names: Vec<(String, Seat)> = opened
        .entries
        .iter()
        .map(|entry| (entry.name.to_owned(), entry.seat()))
        .chain(Board::ALL.map(|board| (board.name().to_owned(), Seat::P0)))
        .chain(screens::ALIASES.map(|(name, _)| (name.to_owned(), Seat::P0)))
        .collect();
    names.sort_by(|a, b| (&a.0, a.1.get()).cmp(&(&b.0, b.1.get())));
    Ok(names)
}

/// The name of every recording, each once whatever seats it has, sorted: the list the testing
/// screen shows (`Input::Recordings`).
///
/// # Errors
///
/// When the pack cannot be built or read.
pub fn listed() -> Result<Vec<String>, String> {
    let mut names: Vec<String> = names()?.into_iter().map(|(name, _)| name).collect();
    names.dedup();
    Ok(names)
}

/// Whether a named scene is built by the inspector and has no entry in the recordings pack.
pub(crate) fn standalone(name: &str) -> bool {
    stories::recording_story(name).is_some() || name.starts_with(stories::PREFIX)
}

/// A guess at how long the recording `name` takes to play for `seat`, for running the longest
/// first: the size of its deflated messages, or the most for a stress board, whose battlefield
/// is at the engine's capacity. Zero when there is no such recording.
#[must_use]
pub fn weight(name: &str, seat: Seat) -> usize {
    if Board::parse(name).is_some() {
        return usize::MAX;
    }
    let Ok(opened) = opened() else {
        return 0;
    };
    opened
        .entries
        .binary_search_by(|e| (e.name, e.seat).cmp(&(name, seat.get())))
        .ok()
        .and_then(|at| opened.entries[at].messages.ok())
        .map_or(0, <[u8]>::len)
}

/// Every recording whose name starts with `prefix`, stress boards and screen recordings
/// included, sorted by name and seat, each with its length and number of views. Finding a
/// length plays the recording through the client, on as many threads as the machine has cores.
///
/// # Errors
///
/// When the pack cannot be built or read.
pub fn list(prefix: &str) -> Result<Vec<Listed>, String> {
    crate::data::checkout()?;
    let opened = opened()?;
    let entries: Vec<&Entry> = opened
        .entries
        .iter()
        .filter(|entry| entry.name.starts_with(prefix))
        .collect();
    // `each` keeps the default panic hook quiet, so a recording the client cannot play prints
    // no panic here; its tests say why.
    let lengths = crate::runner::each(&entries, |entry| length(entry));
    let mut listed: Vec<Listed> = entries
        .iter()
        .zip(lengths)
        .map(|(entry, length_ms)| Listed {
            name: entry.name.to_owned(),
            seat: entry.seat(),
            length_ms,
            views: entry.views,
            failure: entry.messages.err().map(str::to_owned),
        })
        .collect();
    listed.extend(
        (Board::ALL.map(Board::name).into_iter())
            .chain(screens::ALIASES.map(|(name, _)| name))
            .filter(|name| name.starts_with(prefix))
            .map(listed_made),
    );
    listed.sort_by(|a, b| (&a.name, a.seat.get()).cmp(&(&b.name, b.seat.get())));
    Ok(listed)
}

/// The stress board or screen recording `name`, which is seen from the first seat only.
pub(crate) fn listed_made(name: &str) -> Listed {
    let playing = pages::page(name, Side::You, Layout::Desktop, LENGTH_WINDOW);
    let (views, length_ms, failure) = match playing {
        Ok(mut page) => {
            let moments = page.moments();
            let views = moments
                .iter()
                .filter(|(moment, _)| matches!(moment, app_client::driver::Name::View(_)))
                .count();
            let length = moments.last().map(|(_, at)| at.0 / 1_000_000);
            (u32::try_from(views).unwrap_or(u32::MAX), length, None)
        }
        Err(why) => (0, None, Some(why)),
    };
    Listed {
        name: name.to_owned(),
        seat: Seat::P0,
        length_ms,
        views,
        failure,
    }
}

/// The window a recording's length is found in.
const LENGTH_WINDOW: Size = Size {
    width: 1440.0,
    height: 900.0,
};

/// How long `entry` plays with no action taken, in milliseconds, on the desktop layout.
fn length(entry: &Entry) -> Option<u64> {
    let messages = pack::messages(entry).ok()?;
    let recording = Recording(messages);
    // A recording the client cannot play has no length; its tests say why.
    std::panic::catch_unwind(|| {
        let mut page = Page::new(
            crate::data::checkout().ok()?,
            recording.into(),
            Side::You,
            Layout::Desktop,
            LENGTH_WINDOW,
        )
        .ok()?;
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

/// How a read of a missing pack's error begins.
pub const MISSING: &str = "the recordings pack for the present sources is missing";

/// The status `b3d` and `b3d-render` exit with when the pack is missing, so that `just b3d`
/// builds it and runs the command again.
pub const MISSING_STATUS: u8 = 3;

/// The status to exit with for the error `message`: [`MISSING_STATUS`] when it says the pack
/// is missing, else `otherwise`.
#[must_use]
pub fn status(message: &str, otherwise: u8) -> u8 {
    if message.contains(MISSING) {
        MISSING_STATUS
    } else {
        otherwise
    }
}

/// The environment variable selecting a pack made by `record` for a local command.
pub const PACK_ENV: &str = "BERYL_B3D_PACK";

/// The path of the corpus pack for the present sources, whether or not it has been built.
fn current() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os(PACK_ENV) {
        return Ok(path.into());
    }
    let hash = pack_hash(&[])?;
    Ok(pack::path(&pack::cache_dir(), &hash))
}

fn pack_hash(selection: &[String]) -> Result<String, String> {
    let root = root();
    let mut files = pack::sources::sources(&root)?;
    if selection.is_empty() {
        files.push(crate::corpus::PATH.into());
    }
    pack::sources::selection_hash(&root, &files, selection)
}

/// Builds the selected pack with `recorder` if it is missing. An empty selection
/// identifies the checked-in corpus pack used by default.
///
/// # Errors
///
/// When the sources cannot be read or the pack cannot be written.
pub fn build(selection: &[String], recorder: &dyn Recorder) -> Result<PathBuf, String> {
    pack::ensure(&pack::cache_dir(), &pack_hash(selection)?, recorder)
}

fn open() -> Result<Opened, String> {
    let path = current()?;
    if !path.is_file() {
        return Err(format!(
            "{MISSING} ({}); `just b3d record` builds it",
            path.display()
        ));
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    // So that other worktrees' builds keep this pack while this process and its siblings use
    // it.
    pack::mark_used(&path);
    // Kept for the life of the process, so that entries can borrow their names and messages.
    let bytes: &'static [u8] = Vec::leak(bytes);
    let entries = pack::read(bytes).map_err(|why| format!("{}: {why}", path.display()))?;
    Ok(Opened {
        entries,
        parsed: Mutex::new(BTreeMap::new()),
    })
}

/// The repository this crate was built from.
pub fn root() -> PathBuf {
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
#[must_use]
pub fn distance(a: &str, b: &str) -> usize {
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
