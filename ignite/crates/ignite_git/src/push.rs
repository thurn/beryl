//! Pushing a commit to a remote branch, refused unless it fast-forwards. A branch here is a
//! branch name or a full ref name such as `refs/ignite/staging` ([`refname`]).

use std::path::Path;

use ignite_core::ids::Commit;
use ignite_core::refname;
use ignite_core::traits::{Error, Push, Result};

use crate::cmd;

/// Pushes `commit` from `tree` to `branch` on `remote`, without force.
///
/// The outcome is read from `git push --porcelain`, which prints one line per ref the remote
/// answered for. A ref line flagged `!` means the remote refused the update. If the branch
/// moved (`[rejected] (fetch first)` or `(non-fast-forward)`), that is [`Push::Rejected`], and
/// the caller rebases and tries again. Any other refusal, such as `[remote rejected]` from a
/// hook or a protected branch, would be refused again, so it is [`Error::Conflict`]. A failure
/// with no ref line means the conversation with the remote never got that far:
/// [`Error::Unreachable`].
pub fn push(tree: &Path, remote: &str, commit: &Commit, branch: &str) -> Result<Push> {
    push_with(tree, remote, commit, branch, None)
}

/// Pushes `commit` from `tree` to `branch` on `remote` in place of `expected`, the commit the
/// caller last saw there, whether or not `commit` contains it (`--force-with-lease`). If the
/// branch has moved off `expected` since, nothing changes and the answer is [`Push::Rejected`],
/// as for a branch that moved under [`push`]; other answers are as for [`push`].
pub fn replace(
    tree: &Path,
    remote: &str,
    commit: &Commit,
    branch: &str,
    expected: &Commit,
) -> Result<Push> {
    push_with(tree, remote, commit, branch, Some(expected))
}

fn push_with(
    tree: &Path,
    remote: &str,
    commit: &Commit,
    branch: &str,
    expected: Option<&Commit>,
) -> Result<Push> {
    let target = refname::remote_ref(branch);
    let refspec = format!("{commit}:{target}");
    let mut command = cmd::git(tree, ["push", "--porcelain"]);
    if let Some(expected) = expected {
        command.arg(format!("--force-with-lease={target}:{expected}"));
    }
    command.arg(remote).arg(&refspec);
    let out = cmd::output(&mut command)?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout.lines().find(|line| ref_line_for(line, &target));
    match line {
        Some(line) if line.starts_with('!') && tip_moved(summary(line)) => Ok(Push::Rejected {
            reason: summary(line).to_owned(),
        }),
        Some(line) if line.starts_with('!') => Err(Error::Conflict(format!(
            "the remote refused the push to {branch}: {}",
            summary(line)
        ))),
        Some(_) if out.status.success() => Ok(Push::Pushed {
            commit: commit.clone(),
        }),
        _ => Err(Error::Unreachable(cmd::failure(&command, &out))),
    }
}

/// Deletes `branch` on `remote`. A branch the remote does not have is not an error. A remote
/// that cannot be reached is [`Error::Unreachable`].
pub fn delete(tree: &Path, remote: &str, branch: &str) -> Result<()> {
    let target = refname::remote_ref(branch);
    let refspec = format!(":{target}");
    let mut command = cmd::git(tree, ["push", "--porcelain", remote, &refspec]);
    let out = cmd::output(&mut command)?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    if out.status.success() || stderr.contains("remote ref does not exist") {
        return Ok(());
    }
    Err(Error::Unreachable(cmd::failure(&command, &out)))
}

/// Whether a porcelain line reports on `target`. Such a line reads
/// `<flag>\t<from>:<to>\t<summary>`.
fn ref_line_for(line: &str, target: &str) -> bool {
    line.split('\t')
        .nth(1)
        .and_then(|refs| refs.split_once(':'))
        .is_some_and(|(_, to)| to == target)
}

/// Whether a porcelain summary says the branch moved since it was fetched, which a rebase fixes.
/// Git words these `[rejected] (fetch first)`, `[rejected] (non-fast-forward)` and
/// `[rejected] (stale info)`; `[remote rejected]` is the remote's own refusal.
fn tip_moved(summary: &str) -> bool {
    summary.starts_with("[rejected]")
        && ["fetch first", "non-fast-forward", "stale info"]
            .iter()
            .any(|reason| summary.contains(reason))
}

fn summary(line: &str) -> &str {
    line.split('\t').nth(2).unwrap_or(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_ref_and_summary_of_a_porcelain_line() {
        let line = "!\t3a9e4c1:refs/heads/main\t[rejected] (fetch first)";
        assert!(ref_line_for(line, "refs/heads/main"));
        assert!(!ref_line_for(line, "refs/heads/other"));
        assert!(!ref_line_for("To /some/remote", "refs/heads/main"));
        assert_eq!(summary(line), "[rejected] (fetch first)");
    }

    #[test]
    fn only_a_moved_branch_is_worth_retrying() {
        assert!(tip_moved("[rejected] (fetch first)"));
        assert!(tip_moved("[rejected] (non-fast-forward)"));
        assert!(!tip_moved("[remote rejected] (pre-receive hook declined)"));
        assert!(!tip_moved(
            "[remote rejected] (protected branch hook declined)"
        ));
        assert!(!tip_moved("[rejected] (already exists)"));
    }
}
