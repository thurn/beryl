//! Crash records, and the guard that ends the daemon when a background loop dies.
//!
//! The daemon runs several loops on threads of their own (see `main/loops.rs`). A panic in
//! one of them ends only that thread: the process lives on, the login service never restarts
//! it and `ignite doctor` has nothing to report. So every loop runs under [`guard`], which ends
//! the whole process with [`EXIT_CODE`] when its loop stops for any reason, and the service's
//! keep-alive then starts the daemon again.
//!
//! [`install_hook`] makes every panic in the process, from a loop or from a verb that the
//! server catches, leave a [`CrashRecord`] in the `crash` folder of the Ignite home. Records
//! are plain JSON files, `<unix-millis>-<pid>.json`. Whoever files tickets for crashes reads
//! them with [`pending`] and marks each one with [`mark_reported`], which renames it to
//! `<unix-millis>-<pid>.reported.json`.

use std::backtrace::Backtrace;
use std::fs;
use std::io;
use std::panic::{self, AssertUnwindSafe, PanicHookInfo};
use std::path::{Path, PathBuf};
use std::thread;

use ignite_core::time::SystemClock;
use ignite_core::traits::Clock;
use serde::{Deserialize, Serialize};

/// The exit code of a daemon whose background loop stopped: `EX_SOFTWARE`.
pub const EXIT_CODE: i32 = 70;

const SUFFIX: &str = ".json";
const REPORTED_SUFFIX: &str = ".reported.json";

/// One panic: the contents of a file in the `crash` folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashRecord {
    /// When it happened, as an RFC 3339 time.
    pub at: String,
    /// The Ignite commit of the daemon that panicked.
    pub commit: String,
    /// The panicking thread's name, such as `ignite-tip`.
    pub thread: String,
    /// The panic's message.
    pub message: String,
    /// The source file and line of the panic.
    pub location: String,
    /// The backtrace at the panic, as text.
    pub backtrace: String,
}

/// The folder that holds `home`'s crash records.
pub fn record_dir(home: &Path) -> PathBuf {
    home.join("crash")
}

/// Writes `record` into the crash folder under a name that is not taken, and returns its path.
pub fn write(home: &Path, record: &CrashRecord) -> io::Result<PathBuf> {
    let dir = record_dir(home);
    fs::create_dir_all(&dir)?;
    let millis = SystemClock.now().millis();
    let pid = std::process::id();
    let mut path = dir.join(format!("{millis}-{pid}{SUFFIX}"));
    // Two panics in one millisecond of one process, such as two loops dying together. The name
    // is reserved by creating the file, so that two writers never both see it free and the
    // second overwrites the first.
    for extra in 1.. {
        if !reported(&path).exists() {
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => break,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        path = dir.join(format!("{millis}-{pid}-{extra}{SUFFIX}"));
    }
    let partial = path.with_extension("partial");
    fs::write(
        &partial,
        serde_json::to_vec_pretty(record).map_err(io::Error::other)?,
    )?;
    fs::rename(partial, &path)?;
    Ok(path)
}

/// The records that nobody has marked reported yet, oldest first, with their paths.
pub fn pending(home: &Path) -> Vec<(PathBuf, CrashRecord)> {
    let Ok(entries) = fs::read_dir(record_dir(home)) else {
        return Vec::new();
    };
    let mut found: Vec<(PathBuf, CrashRecord)> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(SUFFIX) && !name.ends_with(REPORTED_SUFFIX))
        })
        .filter_map(|path| {
            let record = serde_json::from_slice(&fs::read(&path).ok()?).ok()?;
            Some((path, record))
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

/// Where a record at `path` goes once it is reported.
fn reported(path: &Path) -> PathBuf {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let stem = name.strip_suffix(SUFFIX).unwrap_or(name);
    path.with_file_name(format!("{stem}{REPORTED_SUFFIX}"))
}

/// Marks the record at `path` as reported, by renaming it to `<name>.reported.json`.
pub fn mark_reported(path: &Path) -> io::Result<()> {
    fs::rename(path, reported(path))
}

/// Makes every panic in this process write a record into `home`'s crash folder before the
/// panic hook that was installed earlier (the default one, which prints the message) runs.
/// `commit` is the daemon's version.
pub fn install_hook(home: PathBuf, commit: String) {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        // A failure to write the record must not hide the panic itself.
        let _ = write(&home, &record_of(info, &commit));
        previous(info);
    }));
}

fn record_of(info: &PanicHookInfo<'_>, commit: &str) -> CrashRecord {
    let payload = info.payload();
    let message = payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic with a payload that is not text".to_owned());
    CrashRecord {
        at: SystemClock.now().to_string(),
        commit: commit.to_owned(),
        thread: thread::current().name().unwrap_or("<unnamed>").to_owned(),
        message,
        location: info
            .location()
            .map_or_else(|| "unknown".to_owned(), ToString::to_string),
        backtrace: Backtrace::force_capture().to_string(),
    }
}

/// Runs `body`, a loop that never ends, and calls `stop` once it does end, by returning or by
/// panicking. The panic hook has already written the record by then.
pub fn guard(name: &str, body: impl FnOnce(), stop: impl FnOnce()) {
    let outcome = panic::catch_unwind(AssertUnwindSafe(body));
    match outcome {
        Ok(()) => eprintln!("ignite: the {name} loop ended; the daemon exits"),
        Err(_) => eprintln!("ignite: the {name} loop panicked; the daemon exits"),
    }
    stop();
}

/// Starts `body` as the loop `name` on a thread of the same name. When it stops, the whole
/// process exits with [`EXIT_CODE`], so that the login service starts the daemon again.
pub fn spawn_loop(name: &'static str, body: impl FnOnce() + Send + 'static) {
    let spawned = thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || guard(name, body, || std::process::exit(EXIT_CODE)));
    if let Err(error) = spawned {
        eprintln!("ignite: cannot start the {name} loop: {error}");
        std::process::exit(EXIT_CODE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    use ignite_core::time::Timestamp;

    fn record(message: &str) -> CrashRecord {
        CrashRecord {
            at: "2026-11-17T14:12:00.000Z".to_owned(),
            commit: "abc".to_owned(),
            thread: "ignite-tip".to_owned(),
            message: message.to_owned(),
            location: "src/tip.rs:1:1".to_owned(),
            backtrace: String::new(),
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ignite-crash-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_written_record_is_pending_until_it_is_marked_reported() {
        let home = scratch("pending");
        let first = write(&home, &record("one")).unwrap();
        let second = write(&home, &record("two")).unwrap();
        assert_ne!(first, second, "two records in one millisecond keep apart");
        let pending_messages = |home: &Path| -> Vec<String> {
            pending(home).into_iter().map(|(_, r)| r.message).collect()
        };
        assert_eq!(pending_messages(&home).len(), 2);

        mark_reported(&first).unwrap();
        assert_eq!(pending_messages(&home), ["two"]);
        assert!(!first.exists());
        let names: Vec<String> = fs::read_dir(record_dir(&home))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.iter().any(|n| n.ends_with(".reported.json")),
            "{names:?}"
        );
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn records_written_at_once_never_overwrite_each_other() {
        let home = scratch("together");
        let writers: Vec<_> = (0..16)
            .map(|n| {
                let home = home.clone();
                thread::spawn(move || {
                    let record = CrashRecord {
                        at: SystemClock.now().to_string(),
                        commit: "abc1234".to_owned(),
                        thread: format!("writer-{n}"),
                        message: format!("panic {n}"),
                        location: "here".to_owned(),
                        backtrace: String::new(),
                    };
                    write(&home, &record).unwrap()
                })
            })
            .collect();
        let mut paths: Vec<PathBuf> = writers.into_iter().map(|w| w.join().unwrap()).collect();
        paths.sort();
        paths.dedup();
        assert_eq!(paths.len(), 16);
        assert_eq!(pending(&home).len(), 16);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn a_home_with_no_crash_folder_has_nothing_pending() {
        assert!(pending(&scratch("none")).is_empty());
    }

    #[test]
    fn a_panic_on_a_named_thread_leaves_a_record_that_says_where_and_why() {
        let home = scratch("hook");
        let original = panic::take_hook();
        install_hook(home.clone(), "abc1234".to_owned());
        let outcome = thread::Builder::new()
            .name("ignite-crash-test".to_owned())
            .spawn(|| panic!("the tip loop broke"))
            .unwrap()
            .join();
        // Put back what was there, so that panics in other tests are reported as before.
        drop(panic::take_hook());
        panic::set_hook(original);
        assert!(outcome.is_err());

        let (_, record) = pending(&home)
            .into_iter()
            .find(|(_, record)| record.thread == "ignite-crash-test")
            .expect("a record for the thread");
        assert_eq!(record.message, "the tip loop broke");
        assert_eq!(record.commit, "abc1234");
        assert!(
            // `file!()` separates with backslashes on Windows.
            record
                .location
                .replace('\\', "/")
                .starts_with("crates/ignite_daemon/src/crash.rs:"),
            "{}",
            record.location
        );
        assert!(Timestamp::parse(&record.at).is_ok(), "{}", record.at);
        assert!(record.backtrace.contains("crash"), "{}", record.backtrace);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn a_loop_that_panics_or_returns_is_stopped() {
        let stopped = Cell::new(0);
        guard("test", || panic!("boom"), || stopped.set(stopped.get() + 1));
        assert_eq!(stopped.get(), 1);
        guard("test", || {}, || stopped.set(stopped.get() + 1));
        assert_eq!(stopped.get(), 2);
    }
}
