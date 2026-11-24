//! Building the executable for installation and release updates.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use ignite_core::ids::Commit;
use ignite_core::protocol::Class;
use ignite_core::traits::{CommandSpec, Error, Launcher, Result, RunResult};

use super::{Builder, tail};

/// The `cargo` to run: the one in `~/.cargo/bin` when the service's minimal `PATH` lacks it.
fn cargo_program() -> PathBuf {
    let home = ignite_core::config::home_dir(|name| std::env::var_os(name));
    let cargo = home
        .map(|h| {
            h.join(".cargo")
                .join("bin")
                .join(if cfg!(windows) { "cargo.exe" } else { "cargo" })
        })
        .filter(|p| p.is_file());
    cargo.unwrap_or_else(|| PathBuf::from("cargo"))
}

/// The arguments of the release build of the `ignite` executable in the workspace at `workspace`
/// (`tree/ignite`), without the program.
fn cargo_arguments(workspace: &Path, target: &Path) -> Vec<String> {
    let manifest = workspace.join("Cargo.toml");
    vec![
        "build".to_owned(),
        "--release".to_owned(),
        "--package".to_owned(),
        "ignite_cli".to_owned(),
        "--manifest-path".to_owned(),
        manifest.display().to_string(),
        "--target-dir".to_owned(),
        target.display().to_string(),
    ]
}

/// Where `cargo build --release` leaves the executable in `target`.
fn built_executable(target: &Path) -> PathBuf {
    target
        .join("release")
        .join(format!("ignite{}", std::env::consts::EXE_SUFFIX))
}

/// Runs `cargo build --release` for the `ignite` executable in the workspace at `tree/ignite`,
/// and waits for it on the calling thread.
fn cargo_build(tree: &Path) -> Result<PathBuf> {
    let workspace = tree.join("ignite");
    let target = workspace.join(CACHED_TARGET);
    let output = Command::new(cargo_program())
        .args(cargo_arguments(&workspace, &target))
        .output()
        .map_err(|e| Error::Io(format!("cannot run cargo: {e}")))?;
    if !output.status.success() {
        return Err(Error::Io(format!(
            "cargo build failed in {}:\n{}",
            workspace.display(),
            tail(&String::from_utf8_lossy(&output.stderr))
        )));
    }
    Ok(built_executable(&target))
}

/// Builds in the checkout that `ignite install` runs from.
pub struct CheckoutBuilder {
    pub repo: PathBuf,
}

impl Builder for CheckoutBuilder {
    fn build(&self, _commit: &Commit) -> Result<PathBuf> {
        cargo_build(&self.repo)
    }
}

/// Seconds after which the supervisor ends a release build that has not finished.
pub const RELEASE_BUILD_HANG_LIMIT_S: u64 = 60 * 60;

const CACHED_TARGET: &str = "target";
const RETRY_TARGET: &str = "target-retry";

/// The update tree: one tree of its own, reset to the commit being built, so that the next
/// update starts from the last one's compiled output and never touches a build tree.
pub struct TreeBuilder {
    pub repo: PathBuf,
    /// The directory that holds the update tree.
    pub work: PathBuf,
}

impl TreeBuilder {
    const BRANCH: &'static str = "ignite-update";
    const KEEP: &'static [&'static str] = &["ignite/target"];
    /// The update tree's directory name under [`TreeBuilder::work`].
    pub const TREE: &'static str = "update-tree";

    fn tree(&self) -> PathBuf {
        self.work.join(Self::TREE)
    }

    /// The file that names the commit being built. It stays on disk across a restart, when
    /// Cargo may still be running.
    fn marker(&self) -> PathBuf {
        self.work.join("release-build")
    }

    /// Holds the update tree from before checkout until all build attempts have ended.
    fn lock(&self) -> Result<File> {
        fs::create_dir_all(&self.work)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.work.join("release-build.lock"))?;
        file.lock()?;
        Ok(file)
    }

    /// Makes the update tree a clean checkout of `commit` and notes that a build starts.
    ///
    /// If the previous daemon left a marker, Cargo may still be using either target.
    /// Wait for it before resetting any files in the worktree.
    fn prepare(&self, commit: &Commit) -> Result<PathBuf> {
        let tree = self.tree();
        if self.marker().exists() {
            for target in [CACHED_TARGET, RETRY_TARGET] {
                wait_for_running_build(&tree.join("ignite").join(target))?;
            }
        }
        if tree.join(".git").exists() {
            let keep: Vec<String> = Self::KEEP.iter().map(|s| (*s).to_owned()).collect();
            ignite_git::worktree::reset(&tree, Self::BRANCH, commit, &keep)?;
        } else {
            ignite_git::worktree::add(&self.repo, &tree, Self::BRANCH, commit)?;
        }
        fs::write(self.marker(), commit.as_str())?;
        // Resolve symlinks and Windows junctions so Cargo sees one consistent path for `cwd`,
        // `--manifest-path` and `--target-dir` across incremental release builds.
        Ok(ignite_os::paths::canonicalize(&tree).unwrap_or(tree))
    }

    /// Notes that the build ended, whatever its result.
    fn finished(&self) {
        let _ = fs::remove_file(self.marker());
    }
}

/// Waits for the release profile's Cargo locks. Newer Cargo versions use separate locks
/// for the build and its artifacts.
fn wait_for_running_build(target: &Path) -> Result<()> {
    for name in [".cargo-lock", ".cargo-build-lock", ".cargo-artifact-lock"] {
        match File::open(target.join("release").join(name)) {
            Ok(file) => file.lock_shared()?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// Builds each release in the update tree as a command run by `launcher`, with a hang limit:
/// on the machine's express cores ([`super::direct::ExpressLauncher`]), or through the scheduler as an
/// `agent` command on a machine with none. [`Builder::build`] blocks until Cargo ends, so the
/// caller runs it on a thread of its own (see `SelfUpdate`).
pub struct ReleaseBuilder {
    pub tree: TreeBuilder,
    pub launcher: Arc<dyn Launcher>,
    /// Windows uses a separate log for the retry so the cached build's output survives.
    pub retry_launcher: Option<Arc<dyn Launcher>>,
}

impl ReleaseBuilder {
    /// The command that builds the release in the workspace `tree/ignite`.
    fn command(workspace: &Path, target: &Path, commit: &Commit) -> CommandSpec {
        let mut args = vec![cargo_program().display().to_string()];
        args.extend(cargo_arguments(workspace, target));
        CommandSpec {
            args,
            cwd: workspace.to_owned(),
            env: vec![("IGNITE_RELEASE_BUILD".to_owned(), commit.to_string())],
            class: Class::Agent,
            // The scheduler grants what it can, at most two thirds of the machine.
            cores: std::thread::available_parallelism()
                .map_or(4, |n| u32::try_from(n.get()).unwrap_or(u32::MAX)),
            resources: Vec::new(),
            grows: true,
            tree: None,
            ticket: None,
            hang_limit_s: Some(RELEASE_BUILD_HANG_LIMIT_S),
            reserve: None,
        }
    }

    /// The executable the build left at `executable`, or why it failed.
    fn outcome(result: &RunResult, executable: PathBuf) -> Result<PathBuf> {
        if result.hung {
            return Err(Error::Io(format!(
                "the release build ran longer than {RELEASE_BUILD_HANG_LIMIT_S} seconds and was \
                 ended; its output is in {}",
                result.output.display()
            )));
        }
        if result.exit != Some(0) {
            let text = fs::read(&result.output).unwrap_or_default();
            return Err(Error::Io(format!(
                "the release build failed ({}); its output is in {}:\n{}",
                result
                    .exit
                    .map_or_else(|| "ended by a signal".to_owned(), |c| format!("exit {c}")),
                result.output.display(),
                tail(&String::from_utf8_lossy(&text))
            )));
        }
        Ok(executable)
    }

    fn build_in_tree(&self, workspace: &Path, commit: &Commit) -> Result<PathBuf> {
        let cached = workspace.join(CACHED_TARGET);
        let result = self
            .launcher
            .run(&Self::command(workspace, &cached, commit));
        let failed = match Self::outcome(&result, built_executable(&cached)) {
            Ok(path) => return Ok(path),
            Err(error) => error,
        };
        let Some(retry) = self
            .retry_launcher
            .as_ref()
            .filter(|_| undefined_symbols(&result))
        else {
            return Err(failed);
        };

        eprintln!(
            "ignite: retrying release {commit} with a fresh Cargo target; cached build log: {}",
            result.output.display()
        );
        let target = workspace.join(RETRY_TARGET);
        // The update tree reset keeps the cache and clears any previous retry target.
        // Refuse to reuse a directory if there is one here despite the reset.
        let retried = fs::create_dir(&target)
            .map_err(|error| Error::Io(format!("cannot create {}: {error}", target.display())))
            .and_then(|()| {
                let mut command = Self::command(workspace, &target, commit);
                // Cargo can put intermediate artifacts in a separate configured directory.
                command.env.push((
                    "CARGO_BUILD_BUILD_DIR".to_owned(),
                    target.display().to_string(),
                ));
                let result = retry.run(&command);
                Self::outcome(&result, built_executable(&target))
            });
        retried.map_err(|error| {
            Error::Io(format!(
                "cached attempt failed: {failed}\nfresh target retry failed: {error}"
            ))
        })
    }
}

/// Cargo also fails for compiler errors and timeouts. Only an undefined-symbol diagnostic
/// from a Windows linker calls for a fresh target.
fn undefined_symbols(result: &RunResult) -> bool {
    if result.hung || matches!(result.exit, None | Some(0)) {
        return false;
    }
    let text = fs::read(&result.output).unwrap_or_default();
    String::from_utf8_lossy(&text).lines().any(|line| {
        [
            "rust-lld: error: undefined symbol:",
            "lld-link: error: undefined symbol:",
            "error LNK2001:",
            "error LNK2019:",
        ]
        .iter()
        .any(|diagnostic| line.contains(diagnostic))
    })
}

impl Builder for ReleaseBuilder {
    fn build(&self, commit: &Commit) -> Result<PathBuf> {
        let _lock = self.tree.lock()?;
        let tree = self.tree.prepare(commit)?;
        let outcome = self.build_in_tree(&tree.join("ignite"), commit);
        self.tree.finished();
        outcome
    }
}

#[cfg(test)]
#[path = "build_tests.rs"]
mod tests;
