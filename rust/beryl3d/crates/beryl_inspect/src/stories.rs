//! Finds toolkit stories beside the crates and features they show.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use kit_probe::story::Story;

use crate::recordings;

pub(crate) const PREFIX: &str = "story/";

struct Entry {
    path: PathBuf,
    parsed: OnceLock<Result<Story, String>>,
}

fn catalog() -> Result<&'static BTreeMap<String, Entry>, String> {
    static CATALOG: OnceLock<Result<BTreeMap<String, Entry>, String>> = OnceLock::new();
    CATALOG
        .get_or_init(|| discover(&recordings::root().join("rust/beryl3d/crates")))
        .as_ref()
        .map_err(Clone::clone)
}

fn discover(root: &Path) -> Result<BTreeMap<String, Entry>, String> {
    let mut entries = BTreeMap::new();
    for (name, path) in paths(root)? {
        if path.is_dir() && path.join("Cargo.toml").is_file() {
            walk(&name, &path, false, &mut entries)?;
        }
    }
    Ok(entries)
}

fn paths(path: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let io_error = |error| format!("{}: {error}", path.display());
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(path).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        if !entry.file_type().map_err(io_error)?.is_symlink() {
            let name = entry.file_name().to_string_lossy().into_owned();
            entries.push((name, entry.path()));
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries)
}

fn walk(
    package: &str,
    dir: &Path,
    in_stories: bool,
    entries: &mut BTreeMap<String, Entry>,
) -> Result<(), String> {
    for (name, path) in paths(dir)? {
        if path.is_dir() {
            if !matches!(name.as_str(), "target" | ".git") {
                walk(package, &path, in_stories || name == "stories", entries)?;
            }
        } else if in_stories && path.extension() == Some(OsStr::new("story")) {
            let slug = name.strip_suffix(".story").unwrap_or_default();
            if slug.is_empty()
                || !slug
                    .bytes()
                    .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == b'-')
            {
                return Err(format!(
                    "{}: name a story with lowercase letters, digits or hyphens",
                    path.display()
                ));
            }
            let key = format!("{PREFIX}{package}/{slug}");
            if let Some(previous) = entries.insert(
                key.clone(),
                Entry {
                    path: path.clone(),
                    parsed: OnceLock::new(),
                },
            ) {
                return Err(format!(
                    "two files use {key}: {} and {}",
                    previous.path.display(),
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn names() -> Result<impl Iterator<Item = &'static str>, String> {
    Ok(catalog()?.keys().map(String::as_str))
}

pub(crate) fn open(name: &str) -> Result<&'static Story, String> {
    let catalog = catalog()?;
    let entry = catalog.get(name).ok_or_else(|| {
        let candidates: Vec<_> = catalog.keys().map(String::as_str).collect();
        let nearest = crate::cli::options::nearest(name, &candidates).join(", ");
        format!("no story is named {name:?}; the nearest are {nearest}. `just b3d list story/` lists the stories")
    })?;
    entry
        .parsed
        .get_or_init(|| {
            let source = std::fs::read_to_string(&entry.path)
                .map_err(|why| format!("{}: {why}", entry.path.display()))?;
            source
                .parse::<Story>()
                .map_err(|why| format!("{}: {why}", entry.path.display()))
        })
        .as_ref()
        .map_err(Clone::clone)
}
