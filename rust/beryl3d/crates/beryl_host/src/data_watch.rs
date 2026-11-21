//! Watches the app's runtime tuning away from the frame thread.

use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

use app_client::Input;
use kit_base::data::{Data, profiles};

use crate::Inbox;

/// The runtime tuning at the root of the data directory. The app's reload test starts with
/// this list, the profiles and the effects, and verifies that the runtime loader accepts them.
pub const TUNING_FILES: [&str; 18] = [
    "card.json",
    "badges.json",
    "verbs.json",
    "audit.json",
    "flat.json",
    "hud.json",
    "pointer.json",
    "light.json",
    "own.json",
    "catch-up.json",
    "frame-rate.json",
    "slide.json",
    "tier-probe.json",
    "notices.json",
    "revealed.json",
    "shown.json",
    "gallery.json",
    "lighting.json",
];

/// The files the runtime data loader reads. Build-time constants and other check data are
/// outside the watch. Keep the stamp of a failed read too, so a broken save is reported once.
#[derive(PartialEq, Eq)]
struct Snapshot {
    tuning: Vec<Stamp>,
    effects: Result<Vec<(PathBuf, Stamp)>, ErrorKind>,
}

type Stamp = Result<(SystemTime, u64), ErrorKind>;

fn stamp(path: &Path) -> Stamp {
    let read = || {
        let metadata = fs::metadata(path)?;
        Ok::<_, io::Error>((metadata.modified()?, metadata.len()))
    };
    read().map_err(|error| error.kind())
}

impl Snapshot {
    fn read(directory: &Path) -> Self {
        let tuning = TUNING_FILES
            .into_iter()
            .chain(profiles::FILES)
            .map(|name| stamp(&directory.join(name)))
            .collect();
        let effects = || {
            let mut files = Vec::new();
            for entry in fs::read_dir(directory.join("effects"))? {
                let path = entry?.path();
                if path
                    .extension()
                    .is_some_and(|extension| extension == "json")
                {
                    let stamp = stamp(&path);
                    files.push((path, stamp));
                }
            }
            files.sort_unstable_by(|a, b| a.0.cmp(&b.0));
            Ok::<_, io::Error>(files)
        };
        Self {
            tuning,
            effects: effects().map_err(|error| error.kind()),
        }
    }
}

/// The tuning directory and its last observed file stamps.
pub struct DataWatch {
    directory: PathBuf,
    seen: Snapshot,
}

impl DataWatch {
    /// Takes the baseline stamps before the app reads its initial tuning.
    #[must_use]
    pub fn new(directory: PathBuf) -> Self {
        let seen = Snapshot::read(&directory);
        Self { directory, seen }
    }

    /// Reports a changed set of stamps once, including when a file is removed or restored.
    pub fn changed(&mut self) -> bool {
        let snapshot = Snapshot::read(&self.directory);
        let changed = self.seen != snapshot;
        self.seen = snapshot;
        changed
    }

    /// Polls for edits on a utility thread, validating each changed set before handing it to
    /// the client. Sending via the inbox wakes the host even when playback is paused.
    ///
    /// # Errors
    ///
    /// When the operating system cannot start the thread.
    pub fn start(
        mut self,
        inbox: Inbox,
        load: fn(&Path) -> Result<Data, String>,
    ) -> io::Result<WatchingData> {
        let (stop, stopped) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("beryl3d-data".to_owned())
            .spawn(move || {
                crate::utility_thread();
                while stopped.recv_timeout(Duration::from_millis(250))
                    == Err(RecvTimeoutError::Timeout)
                {
                    if self.changed() {
                        let input = match load(&self.directory) {
                            Ok(data) => Input::Data(Arc::new(data)),
                            Err(error) => Input::DataError(error),
                        };
                        if !inbox.send(input) {
                            break;
                        }
                    }
                }
            })?;
        Ok(WatchingData {
            stop,
            thread: Some(thread),
        })
    }
}

/// The lifetime of a running data watcher. Dropping it wakes and joins its thread.
pub struct WatchingData {
    stop: Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for WatchingData {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
