//! [`FakeGit`], the in-memory repository of [`super`]'s fakes.

use std::collections::{BTreeMap, HashMap};
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

    /// The next rebase's result; by default a rebase is clean onto the tip given.
    pub fn script_rebase(&self, result: Rebase) {
        *lock(&self.rebase) = Some(result);
    }

    /// Every push so far: tree, commit and branch.
    pub fn pushes(&self) -> Vec<(PathBuf, Commit, String)> {
        lock(&self.pushes).clone()
    }
}

impl Git for FakeGit {
    fn object_ids(&self, _tree: &Path, paths: &[String]) -> Result<Vec<(String, Oid)>> {
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
}
