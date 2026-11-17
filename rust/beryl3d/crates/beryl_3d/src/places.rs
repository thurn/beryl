//! Where the app's files go, and the times and checkout its frame log names.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The time, in milliseconds since the Unix epoch.
pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// The checkout the app was built from: four directories above this crate's.
pub(crate) fn checkout() -> String {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.ancestors().nth(4).unwrap_or(manifest);
    root.canonicalize()
        .unwrap_or_else(|_| root.to_owned())
        .display()
        .to_string()
}

/// `$BERYL_CACHE/b3d/frames`, where frame logs go.
pub(crate) fn frames_dir() -> PathBuf {
    cache_dir().join("b3d").join("frames")
}

/// `$BERYL_CACHE/b3d/out`, where the pictures of `just b3d live png` go, as those of
/// `just b3d png` do.
#[cfg(all(feature = "tools", unix))]
pub(crate) fn out_dir() -> PathBuf {
    cache_dir().join("b3d").join("out")
}

/// The cache folder: `~/Library/Caches/beryl`, or on Windows `%LOCALAPPDATA%\beryl`, unless
/// `$BERYL_CACHE` names another.
fn cache_dir() -> PathBuf {
    std::env::var_os("BERYL_CACHE").map_or_else(
        || {
            if cfg!(windows) {
                PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("beryl")
            } else {
                home().join("Library").join("Caches").join("beryl")
            }
        },
        PathBuf::from,
    )
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}
