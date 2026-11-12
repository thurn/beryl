//! The card files of a set crate and the set crates of `rust/cards`, read from the
//! directories.

use std::path::{Path, PathBuf};

/// A card's file, `<set>/<module>.rs` in its crate.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CardFile {
    pub set: String,
    pub module: String,
    pub path: PathBuf,
}

/// The entries of `dir`, sorted by name; none when it is no directory.
fn entries(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| Some((entry.file_name().into_string().ok()?, entry.path())))
        .collect();
    found.sort();
    found
}

/// Every card file of the set crate at `crate_dir`, by set and then module: each `.rs` file
/// in a directory of the crate. The crate's own `lib.rs` and `build.rs` sit beside those
/// directories, in none.
pub fn card_files(crate_dir: &Path) -> Vec<CardFile> {
    let mut found = Vec::new();
    for (set, dir) in entries(crate_dir) {
        for (file, path) in entries(&dir) {
            if let Some(module) = file.strip_suffix(".rs") {
                found.push(CardFile {
                    set: set.clone(),
                    module: module.to_string(),
                    path,
                });
            }
        }
    }
    found
}

/// The set crates under `cards_dir` (`rust/cards`), by name: each directory with a manifest.
pub fn set_crates(cards_dir: &Path) -> Vec<(String, PathBuf)> {
    entries(cards_dir)
        .into_iter()
        .filter(|(_, dir)| dir.join("Cargo.toml").is_file())
        .collect()
}
