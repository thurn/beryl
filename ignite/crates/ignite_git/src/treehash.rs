//! Object IDs of paths in a working tree, uncommitted edits included, through a temporary index
//! so the branch and the real index are never touched.
//!
//! Git already stores a content hash for every file (its blob ID) and directory (its tree ID),
//! so input hashes are built from those instead of reading files. For a tree with uncommitted
//! edits, the working files are added to a copy of the index and git writes the tree object
//! that the copy describes. One `git ls-tree` of that tree then answers every path.
//!
//! The listing is cached per tree. The cache key is `HEAD`, the output of `git status` and the
//! size and modification time of every file that status lists, so editing a file that is
//! already modified, which leaves the status text unchanged, still misses the cache.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::UNIX_EPOCH;

use ignite_core::ids::Oid;
use ignite_core::traits::{Error, Result};

use crate::cmd;

type Listing = Arc<HashMap<String, Oid>>;

/// The last listing computed for each tree, with the key it was computed for.
static CACHE: Mutex<Vec<(PathBuf, String, Listing)>> = Mutex::new(Vec::new());

/// Gives each temporary index in this process a distinct name.
static NEXT_INDEX: AtomicU64 = AtomicU64::new(0);

/// The blob ID of each file path and the tree ID of each directory path, in the order asked.
///
/// Paths are relative to the tree's root, with `/` between parts; `""` or `"."` names the root.
/// A path that does not exist in the working tree, or that only ignored files occupy, is left
/// out of the result.
pub fn object_ids(tree: &Path, paths: &[String]) -> Result<Vec<(String, Oid)>> {
    let listing = listing(tree)?;
    Ok(paths
        .iter()
        .filter_map(|path| {
            let key = path.trim_matches('/');
            let key = if key == "." { "" } else { key };
            listing.get(key).map(|oid| (path.clone(), oid.clone()))
        })
        .collect())
}

/// Every path in the working tree with its object ID, from the cache when the tree is unchanged.
fn listing(tree: &Path) -> Result<Listing> {
    let key = state_key(tree)?;
    {
        let cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((_, _, listing)) = cache.iter().find(|(p, k, _)| p == tree && *k == key) {
            return Ok(Arc::clone(listing));
        }
    }
    let listing = Arc::new(compute(tree)?);
    let mut cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    cache.retain(|(p, _, _)| p != tree);
    cache.push((tree.to_path_buf(), key, Arc::clone(&listing)));
    Ok(listing)
}

/// A string that changes whenever the working tree's content may have changed.
fn state_key(tree: &Path) -> Result<String> {
    let mut key = cmd::run(tree, ["rev-parse", "--verify", "HEAD"])?;
    // `--no-optional-locks` stops status from refreshing and rewriting the real index.
    let status = cmd::run(
        tree,
        [
            "--no-optional-locks",
            "status",
            "--porcelain",
            "-z",
            "--no-renames",
            "--untracked-files=all",
        ],
    )?;
    for entry in status.split('\0').filter(|e| e.len() > 3) {
        let path = &entry[3..];
        write!(key, "\0{entry}").expect("writing to a String cannot fail");
        match fs::symlink_metadata(tree.join(path)) {
            Ok(meta) => {
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_nanos());
                write!(key, "\0{}\0{modified}", meta.len())
                    .expect("writing to a String cannot fail");
            }
            Err(_) => key.push_str("\0-"),
        }
    }
    Ok(key)
}

/// Writes the working tree to a temporary index and lists the tree object it describes.
fn compute(tree: &Path) -> Result<HashMap<String, Oid>> {
    let root = write_working_tree(tree)?;
    let out = cmd::run(
        tree,
        ["ls-tree", "-r", "-t", "-z", "--full-tree", root.as_str()],
    )?;
    let mut listing = HashMap::new();
    for entry in out.split('\0').filter(|e| !e.is_empty()) {
        // Each entry reads "<mode> <type> <oid>\t<path>".
        let (meta, path) = entry
            .split_once('\t')
            .ok_or_else(|| Error::Io(format!("unexpected git ls-tree line: {entry:?}")))?;
        let oid = meta
            .rsplit(' ')
            .next()
            .ok_or_else(|| Error::Io(format!("unexpected git ls-tree line: {entry:?}")))?;
        listing.insert(path.to_owned(), cmd::oid(oid)?);
    }
    listing.insert(String::new(), root);
    Ok(listing)
}

/// The ID of the tree object that the working tree's content makes, uncommitted edits and
/// untracked files included and ignored files left out, as `git add -A` and a commit would
/// make it. It is written through a copy of the index, so the real index is never touched.
pub(crate) fn write_working_tree(tree: &Path) -> Result<Oid> {
    let real = cmd::run(
        tree,
        ["rev-parse", "--path-format=absolute", "--git-path", "index"],
    )?;
    let real = PathBuf::from(real.trim());
    let temporary = TemporaryIndex(PathBuf::from(format!(
        "{}.ignite-{}-{}",
        real.display(),
        std::process::id(),
        NEXT_INDEX.fetch_add(1, Ordering::Relaxed)
    )));
    // Starting from a copy keeps git's cached file sizes and times, so `add -A` hashes only
    // the files that changed.
    if real.exists() {
        fs::copy(&real, &temporary.0)?;
    }
    let mut add = cmd::git(tree, ["add", "--all", "--", "."]);
    add.env("GIT_INDEX_FILE", &temporary.0);
    cmd::checked(&mut add)?;
    let mut write = cmd::git(tree, ["write-tree"]);
    write.env("GIT_INDEX_FILE", &temporary.0);
    cmd::oid(&cmd::checked(&mut write)?)
}

/// A temporary index file, deleted when dropped.
struct TemporaryIndex(PathBuf);

impl Drop for TemporaryIndex {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
