//! Finds stories beside the crates and features they show.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use kit_probe::story::{self, Header, parse_header};

use crate::{recordings, screens, stress::Board};

pub(crate) const PREFIX: &str = "story/";

pub(crate) enum Story {
    Toolkit(story::Story),
    Game(game_probe::story::Story),
}

impl Story {
    pub fn profile(&self) -> Option<&str> {
        match self {
            Self::Toolkit(_) => None,
            Self::Game(story) => story.profile.as_deref(),
        }
    }

    pub fn header(&self) -> Header {
        match self {
            Self::Toolkit(story) => story.header,
            Self::Game(story) => story.header,
        }
    }

    pub fn duration(&self) -> u64 {
        match self {
            Self::Toolkit(story) => story.duration(),
            Self::Game(_) => 0,
        }
    }

    pub fn scale(&self) -> f32 {
        match self {
            Self::Game(game_probe::story::Story {
                body: game_probe::story::Body::Capture { capture, .. },
                ..
            }) => capture.scale,
            Self::Game(game_probe::story::Story {
                body: game_probe::story::Body::Settings(settings),
                ..
            }) => settings.scale.unwrap_or(1.0),
            _ => 1.0,
        }
    }
}

/// Finds the story behind a legacy recording name. Either spelling opens the same replay.
pub(crate) fn recording_story(name: &str) -> Option<&'static str> {
    Board::parse(name).map(Board::story).or_else(|| {
        screens::ALIASES
            .into_iter()
            .find_map(|(legacy, story)| (name == legacy || name == story).then_some(story))
    })
}

pub(crate) fn is_recording_story(name: &str) -> bool {
    name.starts_with(PREFIX) && recording_story(name).is_some()
}

pub(crate) fn is_story(name: &str) -> bool {
    name.starts_with(PREFIX) || has_extension(Path::new(name))
}

pub(crate) fn has_extension(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("story"))
}

struct Entry {
    path: PathBuf,
    parsed: OnceLock<Result<Arc<Story>, String>>,
    feature: Option<String>,
}

fn catalog() -> Result<&'static BTreeMap<String, Entry>, String> {
    static CATALOG: OnceLock<Result<BTreeMap<String, Entry>, String>> = OnceLock::new();
    CATALOG
        .get_or_init(|| discover(&recordings::root().join("client/crates")))
        .as_ref()
        .map_err(Clone::clone)
}

fn discover(root: &Path) -> Result<BTreeMap<String, Entry>, String> {
    let mut entries = BTreeMap::new();
    for (name, path) in paths(root)? {
        if path.is_dir() && path.join("Cargo.toml").is_file() {
            walk(&name, &path, None, &mut entries)?;
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
    story_root: Option<&Path>,
    entries: &mut BTreeMap<String, Entry>,
) -> Result<(), String> {
    for (name, path) in paths(dir)? {
        if path.is_dir() {
            if !matches!(name.as_str(), "target" | ".git") {
                let root = story_root.or_else(|| (name == "stories").then_some(path.as_path()));
                walk(package, &path, root, entries)?;
            }
        } else if let Some(root) = story_root
            && path.extension() == Some(OsStr::new("story"))
        {
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
                    feature: path
                        .parent()
                        .and_then(|parent| parent.strip_prefix(root).ok())
                        .filter(|path| !path.as_os_str().is_empty())
                        .map(|path| path.to_string_lossy().into_owned())
                        .or_else(|| {
                            let parent = root.parent()?.file_name()?.to_str()?;
                            (parent != package).then(|| parent.to_owned())
                        }),
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

pub(crate) fn entries() -> Result<impl Iterator<Item = (&'static str, &'static Path)>, String> {
    Ok(catalog()?
        .iter()
        .map(|(name, entry)| (name.as_str(), entry.path.as_path())))
}

/// Reads the story sources that a tools host makes available to the gallery.
///
/// # Errors
///
/// If the catalog or a story file cannot be read.
pub fn gallery_sources() -> Result<Arc<[app_client::stories::Source]>, String> {
    catalog()?
        .iter()
        .map(|(name, entry)| {
            let text = std::fs::read_to_string(&entry.path)
                .map_err(|why| format!("{}: {why}", entry.path.display()))?;
            Ok(app_client::stories::Source::owned(
                name.clone(),
                text,
                entry.feature.clone(),
            ))
        })
        .collect()
}

pub(crate) fn open(name: &str) -> Result<Arc<Story>, String> {
    let path = Path::new(name);
    if has_extension(path) {
        return read(path).map(Arc::new);
    }
    let catalog = catalog()?;
    let entry = catalog.get(name).ok_or_else(|| {
        let candidates: Vec<_> = catalog.keys().map(String::as_str).collect();
        let nearest = crate::cli::options::nearest(name, &candidates).join(", ");
        format!("no story is named {name:?}; the nearest are {nearest}. `just client list story/` lists the stories")
    })?;
    entry
        .parsed
        .get_or_init(|| read(&entry.path).map(Arc::new))
        .clone()
}

fn read(path: &Path) -> Result<Story, String> {
    let why = |error: String| format!("{}: {error}", path.display());
    let source = std::fs::read_to_string(path).map_err(|error| why(error.to_string()))?;
    let kind = source
        .lines()
        .next()
        .and_then(parse_header)
        .map(|(kind, _)| kind);
    match kind {
        Some("table" | "feature") => game_probe::story::Story::parse(&source)
            .map(Story::Game)
            .map_err(why),
        _ => source
            .parse::<story::Story>()
            .map(Story::Toolkit)
            .map_err(|error| why(error.to_string())),
    }
}
