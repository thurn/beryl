//! Running the `git` executable with an environment that cannot be steered by the caller's.
//!
//! Every call clears the variables that would point git at another repository or index, so a
//! daemon started from inside a git hook still works on the tree it names. Output is in the C
//! locale and git never prompts for credentials, so a remote that wants a password fails at once
//! instead of hanging.

use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Output};

use ignite_core::ids::{Commit, IdError, Oid};
use ignite_core::traits::{Error, Result};

/// Variables that would make git act on something other than the tree it runs in.
const CLEARED: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_PREFIX",
];

/// A `git` command in `dir`, not yet run.
/// Automatic maintenance can hold a fetch open while Git repacks the shared object store.
/// Disable it for Ignite's commands so maintenance can run separately from the daemon's work.
pub(crate) fn git<I, S>(dir: &Path, args: I) -> Command
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new("git");
    command
        .current_dir(dir)
        .args(["-c", "maintenance.auto=false"])
        .args(args);
    for name in CLEARED {
        command.env_remove(name);
    }
    command
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true");
    command
}

/// Runs a command and returns its output whatever its exit status.
pub(crate) fn output(command: &mut Command) -> Result<Output> {
    command
        .output()
        .map_err(|error| Error::Io(format!("cannot run git: {error}")))
}

/// Runs `git <args>` in `dir` and returns its standard output, failing on a non-zero exit.
pub(crate) fn run<I, S>(dir: &Path, args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    checked(&mut git(dir, args))
}

/// Runs a prepared command and returns its standard output, failing on a non-zero exit.
pub(crate) fn checked(command: &mut Command) -> Result<String> {
    let out = output(command)?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    Err(Error::Io(failure(command, &out)))
}

/// A one-line description of a failed command, naming its arguments and git's message.
pub(crate) fn failure(command: &Command, out: &Output) -> String {
    let args: Vec<_> = command.get_args().map(OsStr::to_string_lossy).collect();
    let stderr = String::from_utf8_lossy(&out.stderr);
    format!(
        "git {} failed ({}): {}",
        args.join(" "),
        out.status,
        stderr.trim()
    )
}

pub(crate) fn commit(text: &str) -> Result<Commit> {
    Commit::new(text.trim()).map_err(|e| invalid(&e))
}

pub(crate) fn oid(text: &str) -> Result<Oid> {
    Oid::new(text.trim()).map_err(|e| invalid(&e))
}

fn invalid(error: &IdError) -> Error {
    Error::Invalid(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_names_the_arguments_and_the_message() {
        let dir = ignite_core::test_files::temp_dir();
        let error = run(&dir, ["rev-parse", "--verify", "no-such-ref-anywhere"]).unwrap_err();
        let Error::Io(message) = error else {
            panic!("expected an I/O error, got {error:?}");
        };
        assert!(message.contains("rev-parse --verify no-such-ref-anywhere failed"));
    }
}
