//! Checking `staging` and `master` remote branches in `ignite doctor`: both should be remote
//! branches under `refs/heads/`, fetched in the main checkout, with `master` an ancestor of
//! `staging`, and local branches tracking their remote counterparts.

use std::path::{Path, PathBuf};

use ignite_core::config::Repository;
use ignite_core::ids::Commit;
use ignite_core::refname;
use ignite_core::traits::{Error, Result};

use super::Finding;

/// What `doctor` observes for one configured branch in the main checkout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchState {
    /// The remote-tracking commit last fetched for this branch, or `None` if never fetched.
    pub tip: Option<Commit>,
    /// Whether the corresponding local branch (`refs/heads/<local>`) exists in the main checkout.
    pub local_exists: bool,
    /// The local branch's `(remote, merge)` upstream config, if both keys are set.
    pub upstream: Option<(String, String)>,
}

/// Reads the state of the repository's integration branches for `doctor`.
pub trait BranchSource: Send + Sync {
    fn state(&self, remote: &str, branch: &str) -> Result<BranchState>;
    fn is_ancestor(&self, ancestor: &Commit, descendant: &Commit) -> Result<bool>;
}

/// Reads branch state from the main checkout through `ignite_git`.
pub struct GitBranches {
    pub repo: PathBuf,
}

impl BranchSource for GitBranches {
    fn state(&self, remote: &str, branch: &str) -> Result<BranchState> {
        let tip = match ignite_git::refs::cached_tip(&self.repo, remote, branch) {
            Ok(commit) => Some(commit),
            Err(Error::NotFound(_)) => None,
            Err(error) => return Err(error),
        };
        let local = refname::local_branch(branch);
        let local_ref = format!("refs/heads/{local}");
        let local_exists = ignite_git::refs::local_ref(&self.repo, &local_ref)?.is_some();
        let upstream = if local_exists {
            ignite_git::refs::branch_upstream(&self.repo, &local)?
        } else {
            None
        };
        Ok(BranchState {
            tip,
            local_exists,
            upstream,
        })
    }

    fn is_ancestor(&self, ancestor: &Commit, descendant: &Commit) -> Result<bool> {
        ignite_git::refs::is_ancestor(&self.repo, ancestor, descendant)
    }
}

/// Checks the configured `staging` and `master` branches in the main checkout at `repo`.
pub fn findings(repository: &Repository, repo: &Path) -> Vec<Finding> {
    diagnose(
        repository,
        repo,
        &GitBranches {
            repo: repo.to_path_buf(),
        },
    )
}

/// Pure diagnostic logic over any [`BranchSource`].
pub fn diagnose(repository: &Repository, repo: &Path, source: &dyn BranchSource) -> Vec<Finding> {
    let mut findings = Vec::new();
    let remote = &repository.remote;
    let staging = &repository.staging;
    let master = &repository.master;

    for (role, name) in [("staging", staging.as_str()), ("master", master.as_str())] {
        if !refname::is_branch(name) {
            findings.push(Finding::warning(
                "branches",
                format!(
                    "`repository.{role}` is `{name}`, which is outside `refs/heads/` rather than \
                     a remote branch."
                ),
                format!("set `repository.{role}` to a branch name in ignite.toml"),
            ));
        }
    }

    let staging_state = check_branch(remote, staging, repo, source, &mut findings);
    let master_state = check_branch(remote, master, repo, source, &mut findings);

    if let (Some(master_tip), Some(staging_tip)) = (
        master_state.as_ref().and_then(|s| s.tip.as_ref()),
        staging_state.as_ref().and_then(|s| s.tip.as_ref()),
    ) {
        match source.is_ancestor(master_tip, staging_tip) {
            Ok(true) => {}
            Ok(false) => findings.push(Finding::problem(
                "branches",
                format!(
                    "`{master}` ({}) is not an ancestor of `{staging}` ({}), so tip verification \
                     cannot fast-forward `{master}`.",
                    short(master_tip),
                    short(staging_tip),
                ),
                format!(
                    "inspect `git log --left-right --oneline {}...{}` in {} and reconcile the \
                     branches",
                    refname::tracking_name(remote, master),
                    refname::tracking_name(remote, staging),
                    repo.display(),
                ),
            )),
            Err(error) => findings.push(Finding::problem(
                "branches",
                format!("cannot compare `{master}` and `{staging}`: {error}."),
                format!("check git state in {}", repo.display()),
            )),
        }
    }

    if findings.is_empty() {
        findings.push(Finding::ok(
            "branches",
            format!(
                "`{staging}` and `{master}` track `{remote}` and `{master}` is an ancestor of \
                 `{staging}`."
            ),
        ));
    }
    findings
}

fn check_branch(
    remote: &str,
    branch: &str,
    repo: &Path,
    source: &dyn BranchSource,
    findings: &mut Vec<Finding>,
) -> Option<BranchState> {
    let state = match source.state(remote, branch) {
        Ok(state) => state,
        Err(error) => {
            findings.push(Finding::problem(
                "branches",
                format!("cannot inspect branch `{branch}`: {error}."),
                format!("check git state in {}", repo.display()),
            ));
            return None;
        }
    };
    let tracking = refname::tracking_name(remote, branch);
    if state.tip.is_none() {
        let fetch_arg = if refname::is_branch(branch) {
            branch.to_owned()
        } else {
            format!(
                "{}:{}",
                refname::remote_ref(branch),
                refname::tracking_ref(remote, branch)
            )
        };
        findings.push(Finding::problem(
            "branches",
            format!("remote branch `{branch}` has not been fetched as `{tracking}`."),
            format!(
                "run `git fetch {remote} {fetch_arg}` in {} and verify `{branch}` exists on \
                 `{remote}`",
                repo.display()
            ),
        ));
    }
    if refname::is_branch(branch) {
        let local = refname::local_branch(branch);
        let expected_merge = refname::remote_ref(branch);
        if !state.local_exists {
            findings.push(Finding::warning(
                "branches",
                format!("local branch `{local}` is missing in the main checkout."),
                format!(
                    "run `git branch --track {local} {tracking}` in {}",
                    repo.display()
                ),
            ));
        } else if state
            .upstream
            .as_ref()
            .map(|(r, m)| (r.as_str(), m.as_str()))
            != Some((remote, expected_merge.as_str()))
        {
            let current = match &state.upstream {
                Some((r, m)) => format!("`{r}` -> `{m}`"),
                None => "unset".to_owned(),
            };
            findings.push(Finding::warning(
                "branches",
                format!(
                    "local branch `{local}` upstream is {current} instead of `{remote}` -> \
                     `{expected_merge}`."
                ),
                format!(
                    "run `git branch --set-upstream-to={tracking} {local}` in {}",
                    repo.display()
                ),
            ));
        }
    }
    Some(state)
}

fn short(commit: &Commit) -> &str {
    &commit.as_str()[..commit.as_str().len().min(7)]
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    struct FakeBranches {
        states: BTreeMap<String, BranchState>,
        ancestor: bool,
    }

    impl BranchSource for FakeBranches {
        fn state(&self, _remote: &str, branch: &str) -> Result<BranchState> {
            Ok(self.states.get(branch).cloned().unwrap_or(BranchState {
                tip: None,
                local_exists: false,
                upstream: None,
            }))
        }

        fn is_ancestor(&self, _ancestor: &Commit, _descendant: &Commit) -> Result<bool> {
            Ok(self.ancestor)
        }
    }

    fn repository(staging: &str, master: &str) -> Repository {
        Repository {
            remote: "origin".to_owned(),
            staging: staging.to_owned(),
            master: master.to_owned(),
            trees: PathBuf::from(".worktrees"),
            checks: PathBuf::from("ignite/checks"),
            release: Some("refs/ignite/release".to_owned()),
            release_family: Some("ignite".to_owned()),
            green: Some("refs/ignite/green".to_owned()),
            saved: Some("refs/ignite/tickets/".to_owned()),
            test_paths: Vec::new(),
        }
    }

    fn tracked(commit: &str, merge: &str) -> BranchState {
        BranchState {
            tip: Some(Commit::new(commit).unwrap()),
            local_exists: true,
            upstream: Some(("origin".to_owned(), merge.to_owned())),
        }
    }

    #[test]
    fn healthy_explicit_staging_and_master_branches_report_ok() {
        let source = FakeBranches {
            states: BTreeMap::from([
                (
                    "staging".to_owned(),
                    tracked("bbbbbbb", "refs/heads/staging"),
                ),
                ("master".to_owned(), tracked("aaaaaaa", "refs/heads/master")),
            ]),
            ancestor: true,
        };
        let list = diagnose(
            &repository("staging", "master"),
            Path::new("/repo"),
            &source,
        );
        assert_eq!(list.len(), 1, "{list:?}");
        assert!(list[0].ok && !list[0].warning, "{}", list[0].line());
        assert_eq!(list[0].label, "branches");
    }

    #[test]
    fn reports_missing_remote_branch_diverged_master_and_stale_upstream() {
        let missing = FakeBranches {
            states: BTreeMap::from([(
                "master".to_owned(),
                tracked("aaaaaaa", "refs/heads/master"),
            )]),
            ancestor: true,
        };
        let list = diagnose(
            &repository("staging", "master"),
            Path::new("/repo"),
            &missing,
        );
        assert!(list.iter().any(|f| !f.ok && f.text.contains("staging")));

        let diverged = FakeBranches {
            states: BTreeMap::from([
                (
                    "staging".to_owned(),
                    tracked("bbbbbbb", "refs/ignite/staging"),
                ),
                ("master".to_owned(), tracked("aaaaaaa", "refs/heads/master")),
            ]),
            ancestor: false,
        };
        let list = diagnose(
            &repository("staging", "master"),
            Path::new("/repo"),
            &diverged,
        );
        assert!(
            list.iter()
                .any(|f| !f.ok && f.text.contains("not an ancestor")),
            "{list:?}"
        );
        assert!(
            list.iter()
                .any(|f| f.warning && f.text.contains("refs/ignite/staging")),
            "{list:?}"
        );
    }

    #[test]
    fn warns_when_staging_is_not_a_branch() {
        let source = FakeBranches {
            states: BTreeMap::from([
                (
                    "refs/ignite/staging".to_owned(),
                    tracked("bbbbbbb", "refs/ignite/staging"),
                ),
                ("master".to_owned(), tracked("aaaaaaa", "refs/heads/master")),
            ]),
            ancestor: true,
        };
        let list = diagnose(
            &repository("refs/ignite/staging", "master"),
            Path::new("/repo"),
            &source,
        );
        assert!(
            list.iter()
                .any(|f| f.warning && f.text.contains("outside `refs/heads/`")),
            "{list:?}"
        );
    }
}
