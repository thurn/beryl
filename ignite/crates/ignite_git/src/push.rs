//! Pushing a commit to a remote branch, refused unless it fast-forwards.

use std::path::Path;

use ignite_core::ids::Commit;
use ignite_core::traits::{Error, Push, Result};

use crate::cmd;

/// Pushes `commit` from `tree` to `branch` on `remote`, without force.
///
/// The outcome is read from `git push --porcelain`, which prints one line per ref the remote
/// answered for. A ref line flagged `!` means the remote refused the update, usually because
/// the branch moved: [`Push::Rejected`]. A failure with no ref line means the conversation with
/// the remote never got that far: [`Error::Unreachable`].
pub fn push(tree: &Path, remote: &str, commit: &Commit, branch: &str) -> Result<Push> {
    let target = format!("refs/heads/{branch}");
    let refspec = format!("{commit}:{target}");
    let mut command = cmd::git(tree, ["push", "--porcelain", remote, &refspec]);
    let out = cmd::output(&mut command)?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout.lines().find(|line| ref_line_for(line, &target));
    match line {
        Some(line) if line.starts_with('!') => Ok(Push::Rejected {
            reason: summary(line).to_owned(),
        }),
        Some(_) if out.status.success() => Ok(Push::Pushed {
            commit: commit.clone(),
        }),
        _ => Err(Error::Unreachable(cmd::failure(&command, &out))),
    }
}

/// Whether a porcelain line reports on `target`. Such a line reads
/// `<flag>\t<from>:<to>\t<summary>`.
fn ref_line_for(line: &str, target: &str) -> bool {
    line.split('\t')
        .nth(1)
        .and_then(|refs| refs.split_once(':'))
        .is_some_and(|(_, to)| to == target)
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
}
