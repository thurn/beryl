//! [`FakeGit`], the in-memory repository of [`super`]'s fakes.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::ids::{Commit, Oid};
use crate::traits::{Error, Git, Push, Rebase, Result};

use super::lock;

/// A repository held in maps: object IDs by path, changed paths by commit pair, branch tips.
#[derive(Debug, Default)]
pub struct FakeGit {
    objects: Mutex<BTreeMap<String, Oid>>,
    changes: Mutex<HashMap<(Commit, Commit), Vec<String>>>,
    tips: Mutex<HashMap<String, Commit>>,
    rebase: Mutex<Option<Rebase>>,
    pushes: Mutex<Vec<(PathBuf, Commit, String)>>,
    rejection: Mutex<Option<String>>,
    /// Pairs `(ancestor, descendant)` that are not related; every other pair is.
    unrelated: Mutex<HashSet<(Commit, Commit)>>,
    /// Pairs `(history, version)` where `history` holds `version`'s changes without holding
    /// `version` itself, as a rebased copy does.
    holding: Mutex<HashSet<(Commit, Commit)>>,
    /// How many times `object_ids` was called.
    object_id_calls: std::sync::atomic::AtomicUsize,
}

impl FakeGit {
    /// Makes every later push come back rejected for `reason`, as a push that is not a
    /// fast-forward does. `None` accepts pushes again.
    pub fn reject_pushes(&self, reason: Option<&str>) {
        *lock(&self.rejection) = reason.map(str::to_owned);
    }

    pub fn set_object(&self, path: &str, oid: Oid) {
        lock(&self.objects).insert(path.to_owned(), oid);
    }

    pub fn set_changed(&self, from: &Commit, to: &Commit, paths: &[&str]) {
        let paths = paths.iter().map(|p| (*p).to_owned()).collect();
        lock(&self.changes).insert((from.clone(), to.clone()), paths);
    }

    pub fn set_tip(&self, branch: &str, commit: Commit) {
        lock(&self.tips).insert(branch.to_owned(), commit);
    }

    /// Makes `ancestor` not an ancestor of `descendant`. By default every commit is an
    /// ancestor of every other, as on a branch that only moves forward.
    pub fn set_not_ancestor(&self, ancestor: &Commit, descendant: &Commit) {
        lock(&self.unrelated).insert((ancestor.clone(), descendant.clone()));
    }

    /// Makes `history` hold the changes of `version` though `version` is not its ancestor, as
    /// the rebased copy of a commit does.
    pub fn set_holds(&self, history: &Commit, version: &Commit) {
        lock(&self.holding).insert((history.clone(), version.clone()));
    }

    /// The next rebase's result; by default a rebase is clean onto the tip given.
    pub fn script_rebase(&self, result: Rebase) {
        *lock(&self.rebase) = Some(result);
    }

    /// How many times `object_ids` has been called, each of which reads the whole tree.
    pub fn object_id_calls(&self) -> usize {
        self.object_id_calls
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Every push so far: tree, commit and branch.
    pub fn pushes(&self) -> Vec<(PathBuf, Commit, String)> {
        lock(&self.pushes).clone()
    }
}

impl Git for FakeGit {
    fn object_ids(&self, _tree: &Path, paths: &[String]) -> Result<Vec<(String, Oid)>> {
        self.object_id_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let objects = lock(&self.objects);
        paths
            .iter()
            .map(|path| match objects.get(path) {
                Some(oid) => Ok((path.clone(), oid.clone())),
                None => Err(Error::NotFound(path.clone())),
            })
            .collect()
    }

    fn changed(&self, from: &Commit, to: &Commit) -> Result<Vec<String>> {
        Ok(lock(&self.changes)
            .get(&(from.clone(), to.clone()))
            .cloned()
            .unwrap_or_default())
    }

    fn rebase_onto(&self, _tree: &Path, tip: &Commit) -> Result<Rebase> {
        Ok(lock(&self.rebase).take().unwrap_or_else(|| Rebase::Clean {
            onto: tip.clone(),
            head: tip.clone(),
            landed_since: 0,
        }))
    }

    fn push(&self, tree: &Path, commit: &Commit, branch: &str) -> Result<Push> {
        if let Some(reason) = lock(&self.rejection).clone() {
            return Ok(Push::Rejected { reason });
        }
        lock(&self.pushes).push((tree.to_path_buf(), commit.clone(), branch.to_owned()));
        lock(&self.tips).insert(branch.to_owned(), commit.clone());
        Ok(Push::Pushed {
            commit: commit.clone(),
        })
    }

    fn tip(&self, branch: &str) -> Result<Commit> {
        lock(&self.tips)
            .get(branch)
            .cloned()
            .ok_or_else(|| Error::NotFound(branch.to_owned()))
    }

    fn is_ancestor(&self, ancestor: &Commit, descendant: &Commit) -> Result<bool> {
        Ok(!lock(&self.unrelated).contains(&(ancestor.clone(), descendant.clone())))
    }

    fn holds_changes(&self, history: &Commit, version: &Commit, _path: &str) -> Result<bool> {
        Ok(self.is_ancestor(version, history)?
            || lock(&self.holding).contains(&(history.clone(), version.clone())))
    }

    fn replace(
        &self,
        tree: &Path,
        commit: &Commit,
        branch: &str,
        expected: &Commit,
    ) -> Result<Push> {
        if lock(&self.tips).get(branch) != Some(expected) {
            return Ok(Push::Rejected {
                reason: "[rejected] (stale info)".to_owned(),
            });
        }
        lock(&self.pushes).push((tree.to_path_buf(), commit.clone(), branch.to_owned()));
        lock(&self.tips).insert(branch.to_owned(), commit.clone());
        Ok(Push::Pushed {
            commit: commit.clone(),
        })
    }
}
