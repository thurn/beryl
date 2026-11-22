//! Measure logical bytes and stop cleaning as soon as the tree has room to build again.

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, BufRead, BufReader};
use std::path::{Component, Path, PathBuf};

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
    All,
}

impl Pass {
    fn label(self) -> &'static str {
        match self {
            Self::Replaced => "replaced objects and builds",
            Self::Unused => "unused Cargo fingerprints and outputs",
            Self::Incremental => "incremental caches",
            Self::All => "build directories",
        }
    }

    fn clean(self, dir: &Path, used: &BTreeSet<PathBuf>) -> io::Result<()> {
        match self {
            Self::Replaced => {
                objects::sweep_objects(dir);
                sweep_superseded(dir);
                Ok(())
            }
            Self::Unused => sweep_unused(dir, used),
            Self::Incremental => profile_dirs(dir)
                .iter()
                .try_for_each(|profile| remove_output(&profile.join("incremental"))),
            Self::All => remove_output(dir),
        }
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
    for pass in [Pass::Replaced, Pass::Unused, Pass::Incremental, Pass::All] {
        if before < target {
            break;
        }
        for dir in dirs {
            pass.clean(dir, used)?;
        }
        let after = size(dirs)?;
        if after < before {
            log(pass.label());
        }
        before = after;
    }
    Ok(())
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
