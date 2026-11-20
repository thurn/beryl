//! Reading and fetching refs: a remote branch's tip, a tree's HEAD, commits between two points.

use std::path::Path;

use ignite_core::ids::Commit;
use ignite_core::traits::{Error, Result};

use crate::cmd;

/// Fetches `branch` from `remote` and returns its newest commit.
///
/// The fetch updates the remote-tracking ref `refs/remotes/<remote>/<branch>`, so the commit's
/// objects are present for a later rebase or checkout. A branch the remote does not have is
/// [`Error::NotFound`]; any other failed fetch is [`Error::Unreachable`].
pub fn remote_tip(repo: &Path, remote: &str, branch: &str) -> Result<Commit> {
    let tracking = format!("refs/remotes/{remote}/{branch}");
    let refspec = format!("+refs/heads/{branch}:{tracking}");
    let mut attempt = 0;
    loop {
        let mut fetch = cmd::git(repo, ["fetch", "--quiet", "--no-tags", remote, &refspec]);
        let out = cmd::output(&mut fetch)?;
        if out.status.success() {
            break;
        }
        let message = cmd::failure(&fetch, &out);
        let stderr = String::from_utf8_lossy(&out.stderr);
        if stderr.contains("couldn't find remote ref") {
            return Err(Error::NotFound(message));
        }
        if is_lock_contention(&stderr) {
            // Another fetch in this checkout holds a ref or `FETCH_HEAD` lock. It is not a
            // network failure, and it clears in moments, so wait and try again.
            attempt += 1;
            if attempt >= LOCK_TRIES {
                return Err(Error::Io(message));
            }
            std::thread::sleep(LOCK_PAUSE * attempt);
            continue;
        }
        return Err(Error::Unreachable(message));
    }
    cached_tip(repo, remote, branch)
}

/// How many times a fetch that hit a lock held by another git process is tried, and how long
/// the first pause is (each further pause is one pause longer).
const LOCK_TRIES: u32 = 6;
const LOCK_PAUSE: std::time::Duration = std::time::Duration::from_millis(150);

/// Whether a failed git command's error output says another git process holds a lock:
/// `cannot lock ref '...'` or `Unable to create '....lock': File exists`.
fn is_lock_contention(stderr: &str) -> bool {
    stderr.contains("cannot lock ref")
        || (stderr.contains("Unable to create") && stderr.contains(".lock"))
}

/// The newest commit of `branch` as of the last fetch, without contacting the remote: what the
/// remote-tracking ref `refs/remotes/<remote>/<branch>` holds. A branch never fetched is
/// [`Error::NotFound`].
pub fn cached_tip(repo: &Path, remote: &str, branch: &str) -> Result<Commit> {
    let tracking = format!("refs/remotes/{remote}/{branch}");
    let mut parse = cmd::git(
        repo,
        ["rev-parse", "--verify", &format!("{tracking}^{{commit}}")],
    );
    let out = cmd::output(&mut parse)?;
    if !out.status.success() {
        return Err(Error::NotFound(format!("{tracking} was never fetched")));
    }
    cmd::commit(String::from_utf8_lossy(&out.stdout).trim())
}

/// The commit the local ref `name` points at, or `None` when there is no such ref.
pub fn local_ref(repo: &Path, name: &str) -> Result<Option<Commit>> {
    let mut parse = cmd::git(
        repo,
        [
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{name}^{{commit}}"),
        ],
    );
    let out = cmd::output(&mut parse)?;
    if !out.status.success() {
        return Ok(None);
    }
    cmd::commit(String::from_utf8_lossy(&out.stdout).trim()).map(Some)
}

/// Points the local ref `name` at `commit`, in the repository `tree` belongs to.
pub fn set_local_ref(tree: &Path, name: &str, commit: &Commit) -> Result<()> {
    cmd::run(tree, ["update-ref", name, commit.as_str()]).map(|_| ())
}

/// The commit a tree has checked out.
pub fn head(tree: &Path) -> Result<Commit> {
    cmd::commit(&cmd::run(tree, ["rev-parse", "--verify", "HEAD^{commit}"])?)
}

/// The branch a tree has checked out, or `None` for a detached `HEAD`.
pub fn branch(tree: &Path) -> Result<Option<String>> {
    let mut command = cmd::git(tree, ["symbolic-ref", "--quiet", "--short", "HEAD"]);
    let out = cmd::output(&mut command)?;
    if out.status.success() {
        Ok(Some(String::from_utf8_lossy(&out.stdout).trim().to_owned()))
    } else {
        Ok(None)
    }
}

/// The commits after `from` up to and including `to`, oldest first.
pub fn commits_between(repo: &Path, from: &Commit, to: &Commit) -> Result<Vec<Commit>> {
    let range = format!("{from}..{to}");
    cmd::run(repo, ["rev-list", "--reverse", &range])?
        .lines()
        .map(cmd::commit)
        .collect()
}

/// The subject lines of the commits after `from` up to and including `to`, oldest first.
pub fn subjects_between(repo: &Path, from: &Commit, to: &Commit) -> Result<Vec<String>> {
    let range = format!("{from}..{to}");
    Ok(cmd::run(repo, ["log", "--reverse", "--format=%s", &range])?
        .lines()
        .map(str::to_owned)
        .collect())
}

/// Whether `ancestor` is `descendant` or one of its ancestors (`git merge-base --is-ancestor`).
///
/// Exit status 0 means yes and 1 means no. A commit this repository does
/// not have cannot be in the history of one it has fetched, so an unknown `ancestor` is "no".
/// Any other failure is an error.
pub fn is_ancestor(repo: &Path, ancestor: &Commit, descendant: &Commit) -> Result<bool> {
    let mut command = cmd::git(
        repo,
        [
            "merge-base",
            "--is-ancestor",
            ancestor.as_str(),
            descendant.as_str(),
        ],
    );
    let out = cmd::output(&mut command)?;
    match out.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ if String::from_utf8_lossy(&out.stderr).contains("Not a valid commit name") => Ok(false),
        _ => Err(Error::Io(cmd::failure(&command, &out))),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use super::*;

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .current_dir(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    #[test]
    fn lock_errors_are_told_from_network_errors() {
        assert!(is_lock_contention(
            "error: cannot lock ref 'refs/remotes/origin/staging': is at abc but expected def"
        ));
        assert!(is_lock_contention(
            "fatal: Unable to create '/r/.git/FETCH_HEAD.lock': File exists."
        ));
        assert!(!is_lock_contention(
            "fatal: unable to access 'https://x/': Could not resolve host"
        ));
    }

    #[test]
    fn an_ancestor_is_found_and_a_side_branch_or_unknown_commit_is_not() {
        let dir = ignite_core::test_files::temp_dir().join(format!("ignite-git-ancestor-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "--quiet", "-b", "main"]);
        git(&dir, &["commit", "--quiet", "--allow-empty", "-m", "one"]);
        let one = Commit::new(git(&dir, &["rev-parse", "HEAD"])).unwrap();
        git(&dir, &["commit", "--quiet", "--allow-empty", "-m", "two"]);
        let two = Commit::new(git(&dir, &["rev-parse", "HEAD"])).unwrap();
        git(&dir, &["switch", "--quiet", "-c", "side", one.as_str()]);
        git(&dir, &["commit", "--quiet", "--allow-empty", "-m", "side"]);
        let side = Commit::new(git(&dir, &["rev-parse", "HEAD"])).unwrap();
        let unknown = Commit::new("1234567890abcdef1234567890abcdef12345678").unwrap();

        assert!(is_ancestor(&dir, &one, &two).unwrap());
        assert!(is_ancestor(&dir, &two, &two).unwrap());
        assert!(!is_ancestor(&dir, &two, &one).unwrap());
        assert!(!is_ancestor(&dir, &side, &two).unwrap());
        assert!(!is_ancestor(&dir, &unknown, &two).unwrap());
        let _ = fs::remove_dir_all(&dir);
    }
}
