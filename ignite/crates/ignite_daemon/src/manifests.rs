//! The check manifests: those of the tree a command runs in, and those the long-running daemon
//! holds for the work that has no tree.
//!
//! A command reads the families and products it uses from the tree it runs in ([`in_tree`]): a
//! ticket tree's working directory for `ignite test` and `ignite check`, and the rebased commit
//! checked out in a landing tree for a landing's gate. That is the only manifest that matches
//! the sources being checked. The main checkout's manifests, which verb processes read when they
//! start, and the daemon's, which follow `master`, can be older or newer than the tree, and a
//! command built from them runs other commands than the tree's own (a `cargo nextest` profile
//! the tree's code does not have, or a product built with other arguments than the family that
//! reads it lists with). The check runner opens every tree with the tree's manifests (see
//! `crate::checks::Checks::open`), and the build manager reads a product from the tree it builds
//! in (see `crate::build`). Which families this machine runs comes from the tree too, from its
//! `ignite.toml` ([`machine_families`]).
//!
//! The daemon keeps its own copy for what has no tree: which directories a tree reset keeps,
//! what is prebuilt, the exploration families its idle loop chooses from, and a tree that holds
//! no manifest directory. A change to the manifests builds no release and restarts no daemon
//! (see [`crate::update::scope::ignite_change`]), so those components hold a [`Live`] value, and
//! the main loop calls [`Reload::run`] after fast-forwarding the checkout, so the daemon reads
//! the updated manifests without waiting for a restart.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock};

use ignite_core::config::{Config, Machine, Repository};
use ignite_core::ids::Commit;
use ignite_core::manifest::Manifest;

use crate::trees::Trees;

/// A value shared by every clone, which [`Live::set`] replaces for all of them. A reader takes
/// the current value with [`Live::get`] and keeps it for the rest of its work, so it never sees
/// half of an old manifest and half of a new one.
pub struct Live<T> {
    value: Arc<RwLock<Arc<T>>>,
}

/// The check manifests, replaced when those in the main checkout change.
pub type LiveManifest = Live<Manifest>;

impl<T> Live<T> {
    pub fn new(value: T) -> Live<T> {
        Live {
            value: Arc::new(RwLock::new(Arc::new(value))),
        }
    }

    /// The current value.
    pub fn get(&self) -> Arc<T> {
        self.value
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Replaces the value for every clone.
    pub fn set(&self, value: T) {
        *self.value.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(value);
    }
}

impl<T> Clone for Live<T> {
    fn clone(&self) -> Live<T> {
        Live {
            value: self.value.clone(),
        }
    }
}

impl<T> From<T> for Live<T> {
    fn from(value: T) -> Live<T> {
        Live::new(value)
    }
}

impl<T: fmt::Debug> fmt::Debug for Live<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get().fmt(f)
    }
}

/// Two values are equal when their current values are.
impl<T: PartialEq> PartialEq for Live<T> {
    fn eq(&self, other: &Live<T>) -> bool {
        *self.get() == *other.get()
    }
}

impl<T: Eq> Eq for Live<T> {}

/// Every `*.toml` in `dir`, parsed together. Keys this Ignite does not know are reported on
/// standard error and ignored, since a newer Ignite may know them.
pub fn read_directory(dir: &Path) -> Result<Manifest, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut files = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_some_and(|e| e == "toml") {
            let text =
                std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            files.push((path, text));
        }
    }
    files.sort();
    let (manifest, unknown) =
        Manifest::from_files_with_unknown(&files).map_err(|e| e.to_string())?;
    for key in unknown {
        eprintln!("ignite: ignoring the unknown manifest key {key}; a newer Ignite may know it");
    }
    Ok(manifest)
}

/// The manifests of the tree whose working directory is `root`, from its directory `checks`
/// (`repository.checks`, relative to the tree). `None` when the tree has no such directory, as
/// in a test's tree, where the caller uses the manifests it holds. Manifests the tree has but
/// that cannot be read or parsed are an error: running the commands of other manifests on the
/// tree's sources is what reading the tree's prevents.
pub fn in_tree(root: &Path, checks: &Path) -> Result<Option<Manifest>, String> {
    let dir = root.join(checks);
    if !dir.is_dir() {
        return Ok(None);
    }
    read_directory(&dir).map(Some)
}

/// The families that the `ignite.toml` at `root` lists for the machine `hostname`: `None` when
/// there is no such file, and `Some(None)` when the file has no `families` list for this
/// machine, which means every family. A file that cannot be read or parsed is an error.
///
/// The check runner reads this from each tree it opens, so a tree that adds a family to this
/// machine runs it there, and the heartbeat reads it from the main checkout, so the hub shows
/// the list the machine runs now rather than the one it registered with.
pub fn machine_families(
    root: &Path,
    hostname: &str,
) -> Result<Option<Option<Vec<String>>>, String> {
    Ok(machine_table(root, hostname)?.map(|machine| machine.and_then(|m| m.families)))
}

/// The `[machine.<hostname>]` table of the `ignite.toml` at `root`: `None` when there is no
/// such file, and `Some(None)` when the file does not declare this machine. A file that cannot
/// be read or parsed is an error.
pub fn machine_table(root: &Path, hostname: &str) -> Result<Option<Option<Machine>>, String> {
    let path = root.join("ignite.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let (config, _unknown) =
        Config::parse_with_unknown(&path.to_string_lossy(), &text).map_err(|e| e.to_string())?;
    Ok(Some(config.machine(hostname).cloned()))
}

/// Reads the main checkout's manifests again and hands any change to the daemon's components.
pub struct Reload {
    /// The main checkout.
    pub repo: PathBuf,
    pub config: Config,
    /// The manifests the idle work reads, and the build manager for a tree with no manifest
    /// directory.
    pub manifest: LiveManifest,
    /// The trees, whose reset keeps and prebuild lists come from the products.
    pub trees: Trees,
}

impl Reload {
    /// Reads the manifests from the main checkout. Returns whether they differed from those
    /// held, which are then replaced. Manifests that cannot be read or parsed, such as a
    /// checkout halfway through a merge, leave the held ones in place.
    pub fn run(&self) -> Result<bool, String> {
        let fresh = crate::main::read_manifest(&self.repo, &self.config)?;
        Ok(self.take(fresh))
    }

    /// Holds `fresh` if it differs from the manifests held; returns whether it did.
    pub fn take(&self, fresh: Manifest) -> bool {
        if *self.manifest.get() == fresh {
            return false;
        }
        self.trees.reload_products(&fresh);
        self.manifest.set(fresh);
        true
    }
}

/// Publishes the latest reviewed check declarations for dispatch. The hub runs in a separate
/// process from the daemon that fetches staging, so this reads the locally fetched commit.
pub struct DispatchManifest {
    pub repo: PathBuf,
    pub repository: Repository,
    pub published: Option<Commit>,
}

impl DispatchManifest {
    pub fn refresh(&mut self, conn: &rusqlite::Connection) -> Result<(), String> {
        let config = &self.repository;
        let commit = ignite_git::refs::cached_tip(&self.repo, &config.remote, &config.staging)
            .or_else(|error| match error {
                ignite_core::traits::Error::NotFound(_) => {
                    ignite_git::refs::cached_tip(&self.repo, &config.remote, &config.master)
                }
                other => Err(other),
            })
            .map_err(|error| error.to_string())?;
        if self.published.as_ref() == Some(&commit) {
            return Ok(());
        }
        let files = ignite_git::committed::text_files(&self.repo, &commit, &config.checks, "toml")
            .map_err(|error| error.to_string())?;
        let (manifest, _) =
            Manifest::from_files_with_unknown(&files).map_err(|error| error.to_string())?;
        ignite_hub::routes::dispatch::publish_check_families(conn, &manifest)
            .map_err(|error| error.to_string())?;
        self.published = Some(commit);
        Ok(())
    }
}

#[cfg(test)]
#[path = "manifests_tests.rs"]
mod tests;
