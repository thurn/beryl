//! Opening a repository file from one of Ignite's own tests, and recording that the test opened
//! it, so that the test's input hash covers the file (see "Observed inputs" in
//! `docs/ignite.md`).
//!
//! Each function takes a path relative to the checkout's root. When [`READS_ENV`] names a
//! directory, as it does when Ignite runs the tests, the path is appended to a file in it named
//! after the test's process, one line per read: `<binary id>\t<test name>\t<path>`, with the
//! binary and test as cargo-nextest names them (`NEXTEST_BINARY_ID`, `NEXTEST_TEST_NAME`). The
//! line is written with one `write` to a file opened to append, so lines never interleave.
//!
//! [`temp_dir`] gives each test process a fresh folder for scratch files.
//!
//! A game workspace has a helper of its own that writes the same lines, since Ignite shares no
//! crate with it. A test that reads a repository file some other way is not recorded, and its
//! input hash misses the file.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

/// The variable that names the directory a run's tests record their reads in.
pub const READS_ENV: &str = "IGNITE_READS";

/// The checkout's root: the nearest directory holding `.git` above the test's package, which
/// Cargo names at run time in `CARGO_MANIFEST_DIR`, or above this crate when it does not.
pub fn root() -> io::Result<PathBuf> {
    let start = std::env::var_os("CARGO_MANIFEST_DIR")
        .map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")), PathBuf::from);
    start
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("no checkout holds {}", start.display()),
            )
        })
}

/// The text of the file at `relative`, recorded as read.
pub fn read_to_string(relative: impl AsRef<Path>) -> io::Result<String> {
    fs::read_to_string(open(relative.as_ref())?)
}

/// The bytes of the file at `relative`, recorded as read.
pub fn read(relative: impl AsRef<Path>) -> io::Result<Vec<u8>> {
    fs::read(open(relative.as_ref())?)
}

/// The entries of the directory at `relative`, recorded as read: the directory's git tree
/// then enters the hash, so an added or removed entry changes it.
pub fn read_dir(relative: impl AsRef<Path>) -> io::Result<fs::ReadDir> {
    fs::read_dir(open(relative.as_ref())?)
}

/// The absolute path of `relative`, after recording it.
fn open(relative: &Path) -> io::Result<PathBuf> {
    let text = relative_text(relative)?;
    record(&text)?;
    Ok(root()?.join(relative))
}

/// `relative` with forward slashes; an error for an absolute path or one that climbs.
fn relative_text(relative: &Path) -> io::Result<String> {
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::CurDir => {}
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "{} is not a path inside the checkout, relative to its root",
                        relative.display()
                    ),
                ));
            }
        }
    }
    Ok(parts.join("/"))
}

/// Appends one read to this process's file in the directory [`READS_ENV`] names, if it is set.
fn record(path: &str) -> io::Result<()> {
    let Some(dir) = std::env::var_os(READS_ENV) else {
        return Ok(());
    };
    let binary = std::env::var("NEXTEST_BINARY_ID").unwrap_or_default();
    let test = std::env::var("NEXTEST_TEST_NAME")
        .ok()
        .or_else(|| std::thread::current().name().map(str::to_owned))
        .unwrap_or_default();
    let line = format!("{binary}\t{test}\t{path}\n");
    let file = Path::new(&dir).join(file_name(std::process::id(), &test));
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)?
        .write_all(line.as_bytes())
}

/// The name of a process's file: its ID, which no other process running at the same time has,
/// then the test's name, cut short and with only letters, digits, `-` and `_`, for a reader.
fn file_name(process: u32, test: &str) -> String {
    let readable: String = test
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    format!("{process}-{readable}")
}

/// The folder Ignite's tests make their scratch files and folders under: a folder of the
/// system's temporary directory that belongs to this test process alone, empty when the process
/// first asks for it.
///
/// Tests used to put their folders straight in the temporary directory, named after the process
/// ID. Windows reuses process IDs within minutes, so a later run reopened a folder an earlier run
/// had left behind, and a test that expected an empty database failed on rows from the earlier
/// run. This folder's name also holds the time the process first asked for it, so a later
/// process with the same ID gets another folder. The name is short because macOS limits a
/// socket's path to 104 bytes, and some tests put sockets in folders under this one.
pub fn temp_dir() -> PathBuf {
    static FOLDER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FOLDER
        .get_or_init(|| {
            let seconds = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_secs());
            // Six hex digits of the time repeat only after about 194 days.
            #[allow(clippy::disallowed_methods)] // The one call the configuration allows.
            let folder = std::env::temp_dir().join(format!(
                "i{:x}-{:x}",
                std::process::id(),
                seconds % 0x0100_0000
            ));
            let _ = fs::remove_dir_all(&folder);
            let _ = fs::create_dir_all(&folder);
            folder
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_must_lie_inside_the_checkout() {
        assert_eq!(
            relative_text(Path::new("./a/b.txt")).unwrap(),
            "a/b.txt".to_owned()
        );
        assert!(relative_text(Path::new("../a")).is_err());
        assert!(relative_text(Path::new("/etc/hosts")).is_err());
    }

    #[test]
    fn a_file_is_named_after_its_process_and_test() {
        assert_eq!(file_name(42, "tests::reads$dir"), "42-tests__reads_dir");
    }

    /// The package directory is read at run time, as [`root`] reads it. A test binary that
    /// Ignite copied from another tree's build is current there, and a path compiled into it
    /// with `env!` names the other tree: on 2026-11-20 this test compared that path with the
    /// root and failed an express landing.
    #[test]
    fn the_root_holds_this_crate() {
        let root = root().unwrap();
        let package = std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo names the package");
        assert!(Path::new(&package).starts_with(&root), "{}", root.display());
        assert!(root.join(".git").exists());
    }
}
