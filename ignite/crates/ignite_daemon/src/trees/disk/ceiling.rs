//! Measure logical bytes and stop cleaning as soon as the tree has room to build again.

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, BufRead, BufReader};
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use super::{FINGERPRINTS, delete_unit, objects, profile_dirs, remove_output, sweep_superseded};

pub(in crate::trees) struct Measurement {
    pub bytes: io::Result<u64>,
    /// Fingerprint folders used by the last successful Cargo command, including fresh units.
    pub fingerprints: BTreeSet<PathBuf>,
}

impl Measurement {
    pub(in crate::trees) fn read(dirs: &[PathBuf], log: Option<&Path>) -> Self {
        Self {
            bytes: size(dirs),
            fingerprints: log.map_or_else(BTreeSet::new, fingerprints),
        }
    }
}

/// Outputs are paths within the checkout. Reject symlink ancestors before traversing a path,
/// and count nested outputs only once when measuring.
pub(in crate::trees) fn directories(tree: &Path, outputs: &BTreeSet<String>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    for output in outputs {
        let relative = Path::new(output);
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            continue;
        }
        let path = tree.join(relative);
        if relative.ancestors().skip(1).any(|parent| {
            fs::symlink_metadata(tree.join(parent)).is_ok_and(|meta| meta.is_symlink())
        }) {
            continue;
        }
        dirs.push(path);
    }
    dirs
}

pub(in crate::trees) fn size(dirs: &[PathBuf]) -> io::Result<u64> {
    dirs.iter()
        .filter(|path| {
            !dirs
                .iter()
                .any(|parent| parent != *path && path.starts_with(parent))
        })
        .try_fold(0_u64, |sum, path| {
            bytes(path).map(|bytes| sum.saturating_add(bytes))
        })
}

fn bytes(path: &Path) -> io::Result<u64> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    if meta.is_dir() {
        return fs::read_dir(path)?.try_fold(0_u64, |sum, entry| {
            bytes(&entry?.path()).map(|bytes| sum.saturating_add(bytes))
        });
    }
    Ok(if meta.is_file() { meta.len() } else { 0 })
}

/// Cargo logs the path it opens for every unit's fingerprint, even on a fresh build.
/// Only clean variants of profiles represented in this trace. A Cargo version that emits
/// no identifiable paths, or a build that failed, supplies no evidence to delete variants.
fn fingerprints(log: &Path) -> BTreeSet<PathBuf> {
    let Ok(file) = fs::File::open(log) else {
        return BTreeSet::new();
    };
    let mut used = BTreeSet::new();
    for line in BufReader::new(file)
        .lines()
        .map_while(std::result::Result::ok)
    {
        if line.starts_with("$ ") {
            // Express appends each command to one log.
            used.clear();
        }
        if let Some((_, path)) =
            line.split_once("cargo::core::compiler::fingerprint: fingerprint at: ")
            && let Some(unit) = Path::new(path.trim()).parent()
            && unit.is_absolute()
            && unit
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == FINGERPRINTS)
        {
            used.insert(unit.to_owned());
        }
    }
    used
}

#[derive(Clone, Copy)]
enum Pass {
    Replaced,
    Unused,
    Incremental,
    Profiles,
    All,
}

impl Pass {
    fn label(self) -> &'static str {
        match self {
            Self::Replaced => "replaced objects and builds",
            Self::Unused => "unused Cargo fingerprints and outputs",
            Self::Incremental => "incremental caches",
            Self::Profiles => "least recently built profiles",
            Self::All => "build directories",
        }
    }

    /// Cleans every directory. `excess` is how many bytes must go to get under the target.
    fn clean(self, dirs: &[PathBuf], used: &BTreeSet<PathBuf>, excess: u64) -> io::Result<()> {
        match self {
            Self::Incremental => return trim_incremental(dirs, excess),
            Self::Profiles => return trim_profiles(dirs, excess),
            _ => (),
        }
        for dir in dirs {
            match self {
                Self::Replaced => {
                    objects::sweep_objects(dir);
                    sweep_superseded(dir);
                }
                Self::Unused => sweep_unused(dir, used)?,
                Self::Incremental | Self::Profiles => (),
                Self::All => remove_output(dir)?,
            }
        }
        Ok(())
    }
}

pub(in crate::trees) fn clean(
    dirs: &[PathBuf],
    ceiling: u64,
    used: &BTreeSet<PathBuf>,
    mut log: impl FnMut(&str),
) -> io::Result<()> {
    let target = ceiling.saturating_sub(ceiling / 5);
    let mut before = size(dirs)?;
    if before <= ceiling {
        return Ok(());
    }
    for pass in [
        Pass::Replaced,
        Pass::Unused,
        Pass::Incremental,
        Pass::Profiles,
        Pass::All,
    ] {
        if before < target {
            break;
        }
        pass.clean(dirs, used, before - target + 1)?;
        let after = size(dirs)?;
        if after < before {
            log(pass.label());
        }
        before = after;
    }
    Ok(())
}

/// Deletes the incremental caches used longest ago until `excess` bytes are gone.
///
/// Every edit recompiles the crates it touches, and rustc writes a new session directory in
/// each such crate's cache (`<profile>/incremental/<crate>-<hash>/s-<session>`). Deleting all
/// the caches whenever a tree went over its ceiling made the next build of the crate being
/// worked on compile it from scratch, so the caches of the crates compiled most recently go
/// last. A cache never changes what rustc builds, only how fast, so keeping one cannot
/// change a check's result.
fn trim_incremental(dirs: &[PathBuf], excess: u64) -> io::Result<()> {
    let mut caches = Vec::new();
    for dir in dirs {
        for profile in profile_dirs(dir) {
            let Ok(entries) = fs::read_dir(profile.join("incremental")) else {
                continue;
            };
            for entry in entries {
                caches.push(entry?.path());
            }
        }
    }
    remove_oldest(caches, excess)
}

/// Deletes whole profile directories (`<target>/<profile>` or `<target>/<triple>/<profile>`),
/// the one written longest ago first, until `excess` bytes are gone.
///
/// When the earlier passes left a tree above its cleaning target, the next pass used to delete
/// every build directory, even when the tree was already back under its ceiling. A tree seeded
/// from a donor near the ceiling then lost the copy it had just received, and its next build
/// compiled the whole workspace from nothing. A tree's profiles are mostly independent builds
/// (`debug`, `release`, `clippy`, an iOS simulator target), and a ticket usually builds only one
/// or two of them, so the profiles nobody has built recently go first and the one in use is
/// usually kept. Cargo rebuilds a deleted profile from source the next time a command needs it,
/// so this changes how long a build takes, never what it produces.
fn trim_profiles(dirs: &[PathBuf], excess: u64) -> io::Result<()> {
    let profiles = dirs.iter().flat_map(|dir| profile_dirs(dir)).collect();
    remove_oldest(profiles, excess)
}

/// Deletes `paths` in order of their [`last_written`] time, oldest first, until `excess` bytes
/// are gone.
fn remove_oldest(paths: Vec<PathBuf>, excess: u64) -> io::Result<()> {
    let mut entries = Vec::new();
    for path in paths {
        entries.push((last_written(&path), bytes(&path)?, path));
    }
    entries.sort();
    entries.dedup_by(|a, b| a.2 == b.2);
    let mut freed = 0_u64;
    for (_, size, path) in entries {
        if freed >= excess {
            break;
        }
        remove_output(&path)?;
        freed = freed.saturating_add(size);
    }
    Ok(())
}

/// The newest modification time of a path and its direct children. Rustc adds a session
/// directory to a crate's cache each time it compiles the crate, which updates both.
fn last_written(path: &Path) -> SystemTime {
    let modified = |path: &Path| {
        fs::symlink_metadata(path)
            .and_then(|meta| meta.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH)
    };
    let children = fs::read_dir(path)
        .into_iter()
        .flatten()
        .filter_map(Result::ok);
    children
        .map(|entry| modified(&entry.path()))
        .fold(modified(path), SystemTime::max)
}

fn sweep_unused(target: &Path, used: &BTreeSet<PathBuf>) -> io::Result<()> {
    if used.is_empty() {
        return Ok(());
    }
    for profile in profile_dirs(target) {
        let folder = profile.join(FINGERPRINTS);
        let canonical = ignite_os::paths::canonicalize(&folder).unwrap_or_else(|_| folder.clone());
        if !used
            .iter()
            .any(|unit| unit.parent() == Some(canonical.as_path()))
        {
            continue;
        }
        for entry in fs::read_dir(&folder)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() && !used.contains(&canonical.join(entry.file_name())) {
                delete_unit(&profile, &entry.file_name().to_string_lossy())?;
            }
        }
    }
    Ok(())
}
