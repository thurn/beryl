//! Real paths in the ordinary form every program accepts.
//!
//! On Windows, `std::fs::canonicalize` returns a verbatim path, one that starts `\\?\`
//! (`\\?\C:\Users\me`). Many programs cannot use one: `cmd.exe` cannot run a program named by
//! it, so a build shim that called `ignite.exe` that way exited with code 3 ("The system cannot
//! find the path specified") and no Rust build ran on Windows; and git cannot create
//! directories under one. [`canonicalize`] returns the ordinary form (`C:\Users\me`) instead.
//! Ignite's `clippy.toml` refuses the standard library's `canonicalize` functions, so every
//! call in Ignite comes here.

use std::io;
use std::path::{Path, PathBuf};

/// The longest path Windows programs accept without the verbatim prefix (`MAX_PATH`). A longer
/// path keeps its prefix, since without it the path would not open.
const MAX_PATH: usize = 260;

/// `path` with every link resolved and every `.` and `..` removed, as `std::fs::canonicalize`
/// gives it, but without the `\\?\` prefix that call adds on Windows.
///
/// # Errors
/// `path` does not exist, or a part of it cannot be read.
pub fn canonicalize(path: impl AsRef<Path>) -> io::Result<PathBuf> {
    #[allow(clippy::disallowed_methods)] // The one call the configuration allows.
    let real = std::fs::canonicalize(path)?;
    Ok(without_verbatim_prefix(real))
}

/// `path` without a leading `\\?\` before a drive (`\\?\C:\x` becomes `C:\x`) or a share
/// (`\\?\UNC\server\share` becomes `\\server\share`). Any other path is returned unchanged,
/// including one too long for `MAX_PATH` and a verbatim path to a device. The check is on the
/// text, so it gives the same answer on every host.
pub fn without_verbatim_prefix(path: PathBuf) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path;
    };
    let plain = if let Some(share) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{share}")
    } else if let Some(rest) = text.strip_prefix(r"\\?\")
        && starts_with_drive(rest)
    {
        rest.to_owned()
    } else {
        return path;
    };
    if plain.len() >= MAX_PATH {
        return path;
    }
    PathBuf::from(plain)
}

/// Whether `text` starts with a drive letter and a colon, then a separator or nothing.
fn starts_with_drive(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes.len() == 2 || bytes[2] == b'\\')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> String {
        without_verbatim_prefix(PathBuf::from(text))
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn a_drive_path_and_a_share_lose_the_prefix() {
        assert_eq!(plain(r"\\?\C:\Users\me\ignite.exe"), r"C:\Users\me\ignite.exe");
        assert_eq!(plain(r"\\?\d:"), "d:");
        assert_eq!(plain(r"\\?\UNC\server\share\x"), r"\\server\share\x");
    }

    #[test]
    fn other_paths_are_unchanged() {
        assert_eq!(plain("/var/folders/x"), "/var/folders/x");
        assert_eq!(plain(r"C:\Users\me"), r"C:\Users\me");
        assert_eq!(plain(r"\\?\GLOBALROOT\Device\x"), r"\\?\GLOBALROOT\Device\x");
        assert_eq!(plain(r"\\?\C:x"), r"\\?\C:x");
    }

    #[test]
    fn a_path_too_long_without_the_prefix_keeps_it() {
        let long = format!(r"\\?\C:\{}", "a".repeat(MAX_PATH));
        assert_eq!(plain(&long), long);
    }

    #[test]
    fn a_real_path_has_no_prefix() {
        let real = canonicalize(std::env::current_dir().unwrap()).unwrap();
        assert!(!real.to_string_lossy().starts_with(r"\\?\"), "{}", real.display());
        assert!(real.is_absolute());
    }
}
