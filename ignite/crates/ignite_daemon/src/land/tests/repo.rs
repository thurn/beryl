//! The in-memory repository behind [`super::fakes::TestGit`]: each commit's files and parent,
//! the integration branch's linear history, and what each tree has checked out.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ignite_core::fakes::FakeClock;
use ignite_core::ids::{Commit, Oid};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Clock, Rebase};

use super::fakes::hex;

/// A repository in maps: each commit's files, the integration branch's history, and what each
/// tree has checked out. A ticket tree's branch is its base commit plus its own edits.
#[derive(Default)]
pub struct Repo {
    pub next: u64,
    pub files: HashMap<Commit, BTreeMap<String, Oid>>,
    pub parent: HashMap<Commit, Commit>,
    pub history: Vec<Commit>,
    pub checked_out: HashMap<PathBuf, Commit>,
    pub base: HashMap<PathBuf, Commit>,
    pub edits: HashMap<PathBuf, BTreeMap<String, Oid>>,
    pub conflicts: HashMap<PathBuf, Vec<String>>,
    pub pushes: Vec<(PathBuf, Commit)>,
    /// Commits pushed to a branch other than the integration branch, by branch.
    pub branch_pushes: Vec<(String, Commit)>,
    /// Commits pushed over another branch's commit by `Git::replace`: branch, commit, and the
    /// commit it replaced.
    pub replaces: Vec<(String, Commit, Commit)>,
    /// The tips of branches other than the integration branch; one not here is at the
    /// integration branch's tip.
    pub other_tips: HashMap<String, Commit>,
    /// When each commit was made, by `clock`; a commit made with no clock was made at the epoch.
    pub committed: HashMap<Commit, Timestamp>,
    pub clock: Option<Arc<FakeClock>>,
}

impl Repo {
    pub fn tip(&self) -> Commit {
        self.history.last().cloned().expect("a root commit")
    }

    pub fn commit(&mut self, parent: Option<&Commit>, edits: &BTreeMap<String, Oid>) -> Commit {
        self.next += 1;
        let commit = Commit::new(format!("{:040x}", self.next)).unwrap();
        let mut files = parent.map(|p| self.files[p].clone()).unwrap_or_default();
        files.extend(edits.clone());
        self.files.insert(commit.clone(), files);
        if let Some(parent) = parent {
            self.parent.insert(commit.clone(), parent.clone());
        }
        let now = self
            .clock
            .as_ref()
            .map_or(Timestamp::EPOCH, |clock| clock.now());
        self.committed.insert(commit.clone(), now);
        commit
    }

    /// Where `commit` sits in the integration branch's history, if it is there.
    fn position(&self, commit: &Commit) -> Option<usize> {
        self.history.iter().position(|h| h == commit)
    }

    /// The newest commit of the integration branch's history that `commit` holds: itself, or
    /// the first ancestor on the branch.
    fn on_history(&self, commit: &Commit) -> Commit {
        let mut at = commit.clone();
        while self.position(&at).is_none() {
            match self.parent.get(&at) {
                Some(parent) => at = parent.clone(),
                None => return self.history[0].clone(),
            }
        }
        at
    }

    /// The newest commit both `a` and `b` hold. The integration branch is linear and a ticket
    /// branch grows off it, so that is the older of the two commits where each meets it.
    pub fn merge_base(&self, a: &Commit, b: &Commit) -> Commit {
        let (a, b) = (self.on_history(a), self.on_history(b));
        if self.position(&a) <= self.position(&b) {
            a
        } else {
            b
        }
    }

    /// [`ignite_core::traits::Git::replay`]: `head`'s changes since its merge base with `tip`,
    /// as one new commit on `tip`. No tree's branch, base or checkout changes. A replay of the
    /// commit a tree has checked out conflicts when that tree's rebase would.
    pub fn replay(&mut self, head: &Commit, tip: &Commit) -> Rebase {
        let conflict = self
            .checked_out
            .iter()
            .find(|(_, at)| *at == head)
            .and_then(|(tree, _)| self.conflicts.get(tree));
        if let Some(files) = conflict {
            return Rebase::Conflict {
                files: files.clone(),
            };
        }
        let base = self.merge_base(head, tip);
        let landed_since = self.position(tip).unwrap_or(0) - self.position(&base).unwrap_or(0);
        let replayed = if self.position(head).is_some() {
            // Everything `head` holds is on the integration branch: nothing is left to land.
            tip.clone()
        } else if base == *tip {
            head.clone()
        } else {
            let (old, new) = (&self.files[&base], &self.files[head]);
            let changes: BTreeMap<String, Oid> = new
                .iter()
                .filter(|(path, oid)| old.get(*path) != Some(*oid))
                .map(|(path, oid)| (path.clone(), oid.clone()))
                .collect();
            self.commit(Some(tip), &changes)
        };
        Rebase::Clean {
            onto: tip.clone(),
            head: replayed,
            landed_since: u32::try_from(landed_since).unwrap(),
        }
    }

    /// [`ignite_core::traits::Git::tree_id`]: a digest of the files the tree has checked out.
    pub fn tree_id(&self, tree: &Path) -> String {
        hex(&format!("{:?}", self.files[&self.checked_out[tree]]))
    }
}

pub fn edits(pairs: &[(&str, &str)]) -> BTreeMap<String, Oid> {
    pairs
        .iter()
        .map(|(path, text)| ((*path).to_owned(), Oid::new(hex(text)).unwrap()))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use ignite_core::fakes::FakeClock;
    use ignite_core::time::Timestamp;
    use ignite_core::traits::{Git, Rebase};

    use super::super::fakes::TestGit;

    #[test]
    fn a_replay_rebases_in_the_object_database_and_leaves_the_tree_alone() {
        let clock = Arc::new(FakeClock::new(Timestamp::EPOCH));
        let git = TestGit::with_clock(&[("a.txt", "a")], Some(clock.clone()));
        let tree = Path::new("/trees/tree-01");
        let base = git.tip_now();
        git.start(tree, &[("a.txt", "mine")]);
        let head = git.repo.lock().unwrap().checked_out[tree].clone();
        assert_eq!(git.tree_id(tree).unwrap().len(), 40);

        // Nothing landed since: the replay is the head itself.
        let same = git.replay(tree, &head, &base).unwrap();
        assert_eq!(
            same,
            Rebase::Clean {
                onto: base.clone(),
                head: head.clone(),
                landed_since: 0
            }
        );

        clock.advance(60.0);
        git.land_other(&[("b.txt", "theirs")]);
        let tip = git.tip_now();
        assert_eq!(git.merge_base(tree, &head, &tip).unwrap(), base);
        assert_eq!(
            git.committed_at(tree, &tip).unwrap(),
            Timestamp::EPOCH.plus_seconds(60.0)
        );
        let Rebase::Clean {
            head: replayed,
            landed_since,
            ..
        } = git.replay(tree, &head, &tip).unwrap()
        else {
            panic!("the replay conflicted");
        };
        assert_eq!(landed_since, 1);
        assert_eq!(git.parent_of(&replayed), Some(tip.clone()));
        let files = git.repo.lock().unwrap().files[&replayed].clone();
        assert_eq!(files.len(), 2);
        assert_eq!(git.repo.lock().unwrap().checked_out[tree], head);

        git.repo
            .lock()
            .unwrap()
            .conflicts
            .insert(tree.to_path_buf(), vec!["a.txt".to_owned()]);
        assert_eq!(
            git.replay(tree, &head, &tip).unwrap(),
            Rebase::Conflict {
                files: vec!["a.txt".to_owned()]
            }
        );
    }
}
