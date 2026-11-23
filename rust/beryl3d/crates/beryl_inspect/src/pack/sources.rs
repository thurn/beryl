//! What a recording depends on, and the hash that names the pack built from it.
//!
//! A recording is the engine and session played by `beryl_cli` with the cards that Beryl 3D's
//! recorder, `beryl_record`, hands it as its `Catalog`. So it depends on the source of the engine
//! crates `beryl_record` names (`beryl_cli`, `beryl_session` and `beryl_cards`) and of every crate
//! they reach through a `path` dependency (the engine, the rules and the card sets), and on the
//! data those read: the fixture definitions, the sample decks, the verb data and the card store.
//! Beryl 3D's own crates are not among them, so a change to the client alone keeps the hash, and
//! the pack is read again rather than rebuilt.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::recordings::FIXTURE_DEFINITIONS;

/// The files and directories outside the crates that a recording depends on, from the root of
/// the repository.
const DATA: [&str; 7] = [
    FIXTURE_DEFINITIONS,
    "docs/sample-decks",
    "data/ui-conventions.json",
    "data/card-store.tsv",
    "data/set-groups.json",
    "rust/beryl3d/crates/beryl_record/src",
    "rust/beryl3d/Cargo.lock",
];

/// The crates outside Beryl 3D that `beryl_record` depends on, whose path dependencies the
/// sources follow. Keep this in step with `rust/beryl3d/crates/beryl_record/Cargo.toml`:
/// `beryl_cli` takes its cards as a `Catalog` and does not depend on the card crates, so the
/// cards are only reached through `beryl_cards`.
const RECORDER: [&str; 3] = [
    "rust/crates/beryl_cli",
    "rust/crates/beryl_session",
    "rust/crates/beryl_cards",
];

/// Every file a recording depends on under `root`, the repository, by its path from `root`,
/// sorted.
///
/// # Errors
///
/// When a crate's manifest or a directory cannot be read.
pub fn sources(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut crates = BTreeSet::new();
    let mut pending: Vec<PathBuf> = RECORDER.iter().map(|c| root.join(c)).collect();
    while let Some(dir) = pending.pop() {
        let dir = dir
            .canonicalize()
            .map_err(|e| format!("{}: {e}", dir.display()))?;
        if !crates.insert(dir.clone()) {
            continue;
        }
        let manifest = dir.join("Cargo.toml");
        let text = std::fs::read_to_string(&manifest)
            .map_err(|e| format!("{}: {e}", manifest.display()))?;
        pending.extend(path_dependencies(&text).into_iter().map(|p| dir.join(p)));
    }
    let root = root
        .canonicalize()
        .map_err(|e| format!("{}: {e}", root.display()))?;
    let mut files = BTreeSet::new();
    for dir in &crates {
        walk(dir, &mut files)?;
    }
    for data in DATA {
        walk(&root.join(data), &mut files)?;
    }
    files
        .into_iter()
        .map(|file| {
            file.strip_prefix(&root)
                .map(Path::to_path_buf)
                .map_err(|_| format!("{} is outside {}", file.display(), root.display()))
        })
        .collect()
}

/// The `path` of each dependency in a crate's manifest that its library is built with: its
/// dependencies and build dependencies, for every target, but not its dev dependencies.
fn path_dependencies(manifest: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut counted = false;
    for line in manifest.lines().map(str::trim) {
        if let Some(section) = line.strip_prefix('[') {
            let section = section.trim_end_matches(']');
            counted = (section.ends_with("dependencies") && !section.ends_with("dev-dependencies"))
                || section.contains("dependencies.") && !section.contains("dev-dependencies.");
            continue;
        }
        if !counted || line.starts_with('#') {
            continue;
        }
        if let Some(path) = quoted_after(line, "path") {
            paths.push(path.to_owned());
        }
    }
    paths
}

/// The quoted string that `key = ` is followed by in `line`.
fn quoted_after<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let mut rest = line;
    while let Some(at) = rest.find(key) {
        let before = rest[..at].chars().next_back();
        let after = rest[at + key.len()..].trim_start();
        rest = &rest[at + key.len()..];
        if before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '-') {
            continue;
        }
        if let Some(value) = after.strip_prefix('=') {
            let value = value.trim_start().strip_prefix('"')?;
            return value.find('"').map(|end| &value[..end]);
        }
    }
    None
}

/// Adds `path` to `files` if it is a file, or every file under it if it is a directory, leaving
/// out build output (`target`) and hidden files.
fn walk(path: &Path, files: &mut BTreeSet<PathBuf>) -> Result<(), String> {
    let said = |e: std::io::Error| format!("{}: {e}", path.display());
    let meta = std::fs::metadata(path).map_err(said)?;
    if meta.is_file() {
        files.insert(path.to_path_buf());
        return Ok(());
    }
    for entry in std::fs::read_dir(path).map_err(said)? {
        let entry = entry.map_err(said)?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "target" {
            continue;
        }
        walk(&entry.path(), files)?;
    }
    Ok(())
}

/// The hash of `files` under `root` and of the pack format's [`super::VERSION`]: of each file's
/// path and contents, never its time, as 16
/// hexadecimal digits. It is 64-bit FNV-1a, which needs no dependency and is stable across Rust
/// releases, as the standard library's hasher is not promised to be.
///
/// # Errors
///
/// When a file cannot be read.
pub fn hash(root: &Path, files: &[PathBuf]) -> Result<String, String> {
    selection_hash(root, files, &[])
}

/// Hashes a pack's source files and the ordered arguments selecting its recordings.
///
/// # Errors
///
/// When a source file cannot be read.
pub fn selection_hash(
    root: &Path,
    files: &[PathBuf],
    selection: &[String],
) -> Result<String, String> {
    let mut hash = Fnv::default();
    hash.write(&super::VERSION.to_le_bytes());
    hash.write(&(selection.len() as u64).to_le_bytes());
    for argument in selection {
        hash.write(&(argument.len() as u64).to_le_bytes());
        hash.write(argument.as_bytes());
    }
    for file in files {
        let path = root.join(file);
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        // The path with `/` on every platform, so Windows and macOS agree.
        let name = file.to_string_lossy().replace('\\', "/");
        hash.write(name.as_bytes());
        hash.write(&[0]);
        hash.write(&(bytes.len() as u64).to_le_bytes());
        hash.write(&bytes);
    }
    Ok(format!("{:016x}", hash.0))
}

/// 64-bit FNV-1a.
struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_names_its_library_path_dependencies_but_not_its_dev_dependencies() {
        let manifest = r#"
[dependencies]
beryl_engine = { path = "../beryl_engine" }
# beryl_old = { path = "../beryl_old" }
serde = { version = "1", features = ["derive"] }

[build-dependencies]
beryl_card_build = { version = "0.1", path = "../beryl_card_build" }

[dev-dependencies]
beryl_testing = { path = "../beryl_testing" }

[target.'cfg(windows)'.dependencies]
beryl_win = { path = "../beryl_win", features = ["x"] }
"#;
        assert_eq!(
            path_dependencies(manifest),
            ["../beryl_engine", "../beryl_card_build", "../beryl_win"]
        );
    }
}
