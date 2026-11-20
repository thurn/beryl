//! The object IDs a family's listing hashes cover when the family sets
//! `listing_ignores_comments`: each tracked `.rs` file by its code without comments
//! ([`ignite_core::rust_source::code_id`]), and each directory that holds one by those IDs of
//! the files under it (see "Declaring a family" in `docs/ignite.md`).
//!
//! The checks' own hashes never use these IDs: a comment can still change what a check finds,
//! such as a lint over doc comments.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::sync::{LazyLock, Mutex, PoisonError};

use ignite_core::ids::Oid;
use ignite_core::manifest::Family;
use ignite_core::rust_source;

use super::{Checks, OpenTree};
use crate::server::Context;

/// The code ID of each Rust file this daemon has read, by its git blob ID, so a file is read
/// once however many trees and families hash it.
static CODE_IDS: LazyLock<Mutex<HashMap<Oid, Oid>>> = LazyLock::new(Mutex::default);

/// How many code IDs [`CODE_IDS`] holds before it starts again: several times the Rust files
/// of a checkout, so a daemon that has run for weeks does not keep every version it ever read.
const REMEMBERED: usize = 200_000;

impl Checks {
    /// `ids`, as a listing hash of `family` covers them. For a family that does not set
    /// `listing_ignores_comments` they are returned as they are. Otherwise a tracked `.rs` file
    /// gets its code ID, and a directory that holds a tracked `.rs` file gets an ID made from
    /// every file under it, the `.rs` files by their code IDs. A file that cannot be read, or a
    /// directory whose files git cannot name, keeps its git ID.
    pub(crate) fn listing_ids(
        ctx: &Context,
        tree: &OpenTree,
        family: &Family,
        ids: Vec<(String, Oid)>,
    ) -> Vec<(String, Oid)> {
        if !family.listing_ignores_comments {
            return ids;
        }
        let tracked: HashSet<&str> = tree.tracked.iter().map(String::as_str).collect();
        let mut under: HashMap<String, Vec<&String>> = HashMap::new();
        for (path, _) in &ids {
            if tracked.contains(path.as_str()) {
                continue;
            }
            let files: Vec<&String> = tree
                .tracked
                .iter()
                .filter(|file| within(file, path))
                .collect();
            if files.iter().any(|file| is_rust(file)) {
                under.insert(path.clone(), files);
            }
        }
        let mut wanted: Vec<String> = under.values().flatten().map(|f| (*f).clone()).collect();
        wanted.sort();
        wanted.dedup();
        let inner: HashMap<String, Oid> = if wanted.is_empty() {
            HashMap::new()
        } else {
            ctx.git
                .object_ids(&tree.path, &wanted)
                .map(|found| found.into_iter().collect())
                .unwrap_or_default()
        };
        let code = |path: &str, oid: &Oid| -> Oid {
            if is_rust(path) {
                code_id(tree, path, oid)
            } else {
                oid.clone()
            }
        };
        ids.into_iter()
            .map(|(path, oid)| {
                if tracked.contains(path.as_str()) {
                    let id = code(&path, &oid);
                    return (path, id);
                }
                let Some(files) = under.get(path.as_str()) else {
                    return (path, oid);
                };
                let mut parts = Vec::with_capacity(files.len());
                for file in files {
                    let Some(blob) = inner.get(file.as_str()) else {
                        // Git does not name every file under it: keep the directory's own ID.
                        return (path, oid);
                    };
                    parts.push(((*file).clone(), code(file, blob)));
                }
                let hash = Self::sources_hash(&parts);
                let id = Oid::new(hash.as_str()).unwrap_or(oid);
                (path, id)
            })
            .collect()
    }
}

/// The code ID of the tracked Rust file `path`, whose git blob ID is `blob`; `blob` itself
/// when the file cannot be read as text.
fn code_id(tree: &OpenTree, path: &str, blob: &Oid) -> Oid {
    if let Some(known) = CODE_IDS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(blob)
    {
        return known.clone();
    }
    let Ok(text) = fs::read_to_string(tree.path.join(path)) else {
        return blob.clone();
    };
    let id = rust_source::code_id(&text);
    let mut known = CODE_IDS.lock().unwrap_or_else(PoisonError::into_inner);
    if known.len() >= REMEMBERED {
        known.clear();
    }
    known.insert(blob.clone(), id.clone());
    id
}

fn is_rust(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("rs"))
}

/// Whether `path` lies below the directory `dir`; every path lies in the checkout's root, `""`.
fn within(path: &str, dir: &str) -> bool {
    let dir = dir.trim_end_matches('/');
    dir.is_empty() || dir == "." || path.strip_prefix(dir).is_some_and(|r| r.starts_with('/'))
}
