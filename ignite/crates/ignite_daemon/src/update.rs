//! Installing Ignite on a machine and keeping it current: building the executable from each
//! release, restarting into it, and registering with the hub.
//!
//! Every build is stored under the commit it was built from, at `bin/<commit>/ignite` in the
//! machine's Ignite home, and a stored file is never replaced. A supervisor that is still running
//! a command keeps its executable open from that path, so an update only adds a new directory and
//! repoints `bin/current`, the path the login service starts. The daemon then restarts by
//! replacing its own process image with the new executable ([`ExecRestart`]); supervisors are
//! other processes and keep running the old one until their commands finish.
//!
//! Windows differs in three ways. The executable is `ignite.exe`. Symbolic links need a
//! privilege most users lack, so the service starts `bin/current.cmd`, a batch file that never
//! changes and runs the executable named in the text file `bin/current`; an update replaces
//! that text file. And Windows has no `exec`, so a restart starts the new executable as a new
//! process and exits.
//!
//! The logic takes its collaborators as traits ([`Builder`], [`Registrar`], [`Restart`] and the
//! core `Git` and the OS `Service`), so the tests run it against fakes.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use ignite_core::ids::{Commit, MachineId};
use ignite_core::protocol::Class;
use ignite_core::protocol::hub::{MachineRegister, MachineRegistered};
use ignite_core::traits::{CommandSpec, Error, Git, Launcher, Result, RunResult};
#[cfg(windows)]
use ignite_os::Native;
use serde::{Deserialize, Serialize};

mod install;
mod lastgood;
mod rollback;
pub mod selftest;
mod starts;

pub use install::{
    BUILT_VARIABLE, Handoff, HubFallback, InstallDeps, InstallVerb, Installed, LocalHub, Outcome,
    PathLink, ProcessHandoff, ServedHub, finish_install, install, install_checkout, link_onto_path,
    main_checkout,
};
pub use lastgood::{CRASH_STARTS, CrashLoop, LAST_GOOD_AFTER_S, detect_crash_loop, roll_back};
pub use rollback::{CRASH_WINDOW_S, HEALTHY_AFTER_S, KEEP_EXTRA, Startup};
pub use starts::{StartRecord, forget_start, guard_start, spawn_health_marks, start_log};

/// The argument the monitor's login service entry starts the executable with.
pub const MONITOR_VERB: &str = "monitor";

/// The directory of Ignite's own source in the repository; a change under it means a new build,
/// and a landing that touches it may move the release branch.
pub const SOURCE: &str = "ignite";

/// The arguments the login service starts the executable with.
pub const DAEMON_ARGS: &[&str] = &["daemon"];

/// Files that belong to the machine, not to the repository: the stored executables and the
/// machine's identity.
#[derive(Clone, Debug)]
pub struct Home {
    pub root: PathBuf,
}

impl Home {
    /// `$IGNITE_HOME`, or `~/.config/ignite`.
    pub fn from_env() -> Home {
        let root = std::env::var_os("IGNITE_HOME").map_or_else(
            || {
                let home = std::env::var_os("HOME").unwrap_or_default();
                PathBuf::from(home).join(".config").join("ignite")
            },
            PathBuf::from,
        );
        Home { root }
    }

    /// Creates the home folder, readable by its user only: it holds the daemon's socket and
    /// the machine's write token. An existing folder is made private too.
    pub fn create(&self) -> io::Result<()> {
        create_private_dir(&self.root)
    }

    pub fn store(&self) -> ExecutableStore {
        ExecutableStore {
            root: self.root.join("bin"),
        }
    }

    /// The file holding the path of the checkout `ignite install` was run from, which the
    /// daemon serves when the login service starts it outside any checkout.
    pub fn checkout_file(&self) -> PathBuf {
        self.root.join("checkout")
    }

    pub fn identity(&self) -> IdentityFile {
        IdentityFile {
            path: self.root.join("machine.json"),
        }
    }
}

/// Creates `dir` and any missing parents; `dir` itself gets mode 0700 on macOS.
pub fn create_private_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        if let Some(parent) = dir.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match std::fs::DirBuilder::new().mode(0o700).create(dir) {
            Err(error) if error.kind() != io::ErrorKind::AlreadyExists => return Err(error),
            _ => {}
        }
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
    }
    #[cfg(windows)]
    {
        // The folder inherits the user profile's permissions, which allow only this user.
        std::fs::create_dir_all(dir)
    }
}

/// Built executables, one directory per commit.
#[derive(Clone, Debug)]
pub struct ExecutableStore {
    root: PathBuf,
}

impl ExecutableStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Where the executable built from `commit` lives.
    pub fn path(&self, commit: &Commit) -> PathBuf {
        self.root
            .join(commit.as_str())
            .join(format!("ignite{}", std::env::consts::EXE_SUFFIX))
    }

    /// What the login service starts; it always runs the newest stored build. A symbolic link
    /// to it, or on Windows the launcher `CURRENT_CMD`.
    pub fn current_link(&self) -> PathBuf {
        #[cfg(unix)]
        return self.root.join("current");
        #[cfg(windows)]
        return self.root.join("current.cmd");
    }

    pub fn has(&self, commit: &Commit) -> bool {
        self.path(commit).is_file()
    }

    /// Copies `built` to the commit's path and returns that path. A commit that is already
    /// stored keeps its file: it may be running.
    pub fn store(&self, commit: &Commit, built: &Path) -> io::Result<PathBuf> {
        let dest = self.path(commit);
        if dest.is_file() {
            return Ok(dest);
        }
        let dir = dest.parent().expect("a stored path has a directory");
        std::fs::create_dir_all(dir)?;
        let partial = dir.join(format!("ignite.partial-{}", std::process::id()));
        std::fs::copy(built, &partial)?;
        Self::sync_rename(&partial, &dest)?;
        Ok(dest)
    }

    /// Points [`Self::current_link`] at the commit's executable, replacing the link in one step.
    /// The release it pointed at stays stored as the previous one, and the new release starts
    /// on trial (see `rollback`).
    pub fn point_current_at(&self, commit: &Commit) -> io::Result<()> {
        if !self.has(commit) {
            // A link to a build that is not stored would leave the login service with nothing
            // to start.
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("the build of {commit} is not stored"),
            ));
        }
        self.begin_trial(commit);
        self.flip(commit)
    }

    fn flip(&self, commit: &Commit) -> io::Result<()> {
        #[cfg(windows)]
        {
            let launcher = self.current_link();
            if std::fs::read_to_string(&launcher).ok().as_deref() != Some(CURRENT_CMD) {
                std::fs::write(&launcher, CURRENT_CMD)?;
            }
        }
        self.link("current", commit)
    }

    /// Makes `bin/<name>` point at the commit's executable, replacing it in one step: a symbolic
    /// link, or on Windows a text file holding the path, as `current` is.
    fn link(&self, name: &str, commit: &Commit) -> io::Result<()> {
        let partial = self
            .root
            .join(format!("{name}.partial-{}", std::process::id()));
        // A partial file left by a crash would make `symlink` fail.
        let _ = std::fs::remove_file(&partial);
        #[cfg(unix)]
        std::os::unix::fs::symlink(self.path(commit), &partial)?;
        #[cfg(windows)]
        std::fs::write(&partial, self.path(commit).display().to_string())?;
        std::fs::rename(&partial, self.root.join(name))
    }

    /// The commit an executable path was stored under, following symlinks and, on Windows, the
    /// launcher's pointer file.
    pub fn version_of(executable: &Path) -> Option<Commit> {
        let mut real = executable.canonicalize().ok()?;
        if cfg!(windows) && real.extension().is_some_and(|e| e == "cmd") {
            let pointer = std::fs::read_to_string(real.with_extension("")).ok()?;
            real = PathBuf::from(pointer.trim());
        }
        let name = real.parent()?.file_name()?.to_str()?;
        Commit::new(name).ok()
    }
}

/// The Windows launcher at `bin/current.cmd`: it runs the executable whose path is the text of
/// `bin/current`, beside it, with the launcher's arguments. It never changes once written, which
/// matters because `cmd` rereads a batch file line by line while running it.
#[cfg(windows)]
pub const CURRENT_CMD: &str = "@for /f \"usebackq delims=\" %%E in (\"%~dp0current\") do @\"%%E\" %*\r\n\
                               @exit /b %ERRORLEVEL%\r\n";

/// Makes the executable for a commit.
pub trait Builder {
    /// Builds the executable that `commit` contains and returns the path of the built file.
    fn build(&self, commit: &Commit) -> Result<PathBuf>;
}

/// The `cargo` to run: the one in `~/.cargo/bin` when the service's minimal `PATH` lacks it.
fn cargo_program() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let cargo = home
        .map(|h| h.join(".cargo").join("bin").join("cargo"))
        .filter(|p| p.is_file());
    cargo.unwrap_or_else(|| PathBuf::from("cargo"))
}

/// The arguments of the release build of the `ignite` executable in the workspace at `workspace`
/// (`tree/ignite`), without the program.
fn cargo_arguments(workspace: &Path) -> Vec<String> {
    let manifest = workspace.join("Cargo.toml");
    let target = workspace.join("target");
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

/// Where `cargo build --release` leaves the executable, for the workspace at `tree/ignite`.
fn built_executable(workspace: &Path) -> PathBuf {
    workspace
        .join("target")
        .join("release")
        .join(format!("ignite{}", std::env::consts::EXE_SUFFIX))
}

/// The last lines of `text`, for an error message.
fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().rev().take(15).collect();
    lines.into_iter().rev().collect::<Vec<_>>().join("\n")
}

/// Runs `cargo build --release` for the `ignite` executable in the workspace at `tree/ignite`,
/// and waits for it on the calling thread.
fn cargo_build(tree: &Path) -> Result<PathBuf> {
    let workspace = tree.join("ignite");
    let output = Command::new(cargo_program())
        .args(cargo_arguments(&workspace))
        .output()
        .map_err(|e| Error::Io(format!("cannot run cargo: {e}")))?;
    if !output.status.success() {
        return Err(Error::Io(format!(
            "cargo build failed in {}:\n{}",
            workspace.display(),
            tail(&String::from_utf8_lossy(&output.stderr))
        )));
    }
    Ok(built_executable(&workspace))
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

    fn tree(&self) -> PathBuf {
        self.work.join("update-tree")
    }

    /// The file that names the commit whose build was started and has not been seen to end. A
    /// daemon that restarts while a release builds finds it, because the build's supervisor
    /// keeps running.
    fn marker(&self) -> PathBuf {
        self.work.join("release-build")
    }

    /// Makes the update tree a clean checkout of `commit` and notes that a build starts.
    ///
    /// When the file from [`Self::marker`] exists, the previous daemon's build may still run in
    /// the tree, so this first waits until Cargo has released its lock on the target directory.
    /// Resetting the tree under a running build would corrupt it.
    fn prepare(&self, commit: &Commit) -> Result<PathBuf> {
        let tree = self.tree();
        if self.marker().exists() {
            wait_for_running_build(&tree.join("ignite"));
        }
        std::fs::create_dir_all(&self.work)?;
        if tree.join(".git").exists() {
            let keep: Vec<String> = Self::KEEP.iter().map(|s| (*s).to_owned()).collect();
            ignite_git::worktree::reset(&tree, Self::BRANCH, commit, &keep)?;
        } else {
            ignite_git::worktree::add(&self.repo, &tree, Self::BRANCH, commit)?;
        }
        std::fs::write(self.marker(), commit.as_str())?;
        Ok(tree)
    }

    /// Notes that the build ended, whatever its result.
    fn finished(&self) {
        let _ = std::fs::remove_file(self.marker());
    }
}

/// Blocks until no Cargo holds the lock on `workspace`'s release target directory.
fn wait_for_running_build(workspace: &Path) {
    let lock = workspace.join("target").join("release").join(".cargo-lock");
    if let Ok(file) = std::fs::File::open(lock) {
        let _ = file.lock_shared();
    }
}

/// Builds each release in the update tree as a command run by the daemon's launcher, in the
/// `agent` class with a hang limit, so that the scheduler accounts for its cores and its
/// supervisor, not a thread of the daemon, owns the Cargo process. [`Builder::build`] blocks
/// until Cargo ends, so the caller runs it on a thread of its own (see `SelfUpdate`).
pub struct ReleaseBuilder {
    pub tree: TreeBuilder,
    pub launcher: Arc<dyn Launcher>,
}

impl ReleaseBuilder {
    /// The command that builds the release in the workspace `tree/ignite`.
    fn command(tree: &Path, commit: &Commit) -> CommandSpec {
        let workspace = tree.join("ignite");
        let mut args = vec![cargo_program().display().to_string()];
        args.extend(cargo_arguments(&workspace));
        CommandSpec {
            args,
            cwd: workspace,
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
            let text = std::fs::read_to_string(&result.output).unwrap_or_default();
            return Err(Error::Io(format!(
                "the release build failed ({}); its output is in {}:\n{}",
                result
                    .exit
                    .map_or_else(|| "ended by a signal".to_owned(), |c| format!("exit {c}")),
                result.output.display(),
                tail(&text)
            )));
        }
        Ok(executable)
    }
}

impl Builder for ReleaseBuilder {
    fn build(&self, commit: &Commit) -> Result<PathBuf> {
        let tree = self.tree.prepare(commit)?;
        let result = self.launcher.run(&Self::command(&tree, commit));
        self.tree.finished();
        Self::outcome(&result, built_executable(&tree.join("ignite")))
    }
}

/// The hub's side of `ignite install`.
pub trait Registrar {
    /// Registers the machine. `token` is the write token the machine already holds, if any: the
    /// hub refuses to register again a machine that has one unless it is sent.
    fn register(&self, request: &MachineRegister, token: Option<&str>)
    -> Result<MachineRegistered>;
}

/// Registers over HTTP: `POST <url>/<project>/api/machines/register`.
pub struct HttpRegistrar {
    pub url: String,
    pub project: String,
}

impl Registrar for HttpRegistrar {
    fn register(
        &self,
        request: &MachineRegister,
        token: Option<&str>,
    ) -> Result<MachineRegistered> {
        let url = format!(
            "{}/{}/api/machines/register",
            self.url.trim_end_matches('/'),
            self.project
        );
        let body = serde_json::to_string(request).map_err(|e| Error::Invalid(e.to_string()))?;
        let mut post = ureq::post(&url).header("Content-Type", "application/json");
        if let Some(token) = token {
            post = post.header("Authorization", format!("Bearer {token}"));
        }
        let mut response = post.send(body).map_err(|e| match e {
            ureq::Error::StatusCode(status @ (401 | 403)) => Error::Unreachable(format!(
                "{url}: the hub refused this machine's token ({status}); {RECOVER}"
            )),
            e => Error::Unreachable(format!("{url}: {e}")),
        })?;
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|e| Error::Unreachable(format!("{url}: {e}")))?;
        serde_json::from_str(&text).map_err(|e| {
            Error::Invalid(format!(
                "the hub's answer to {url} is not a registration: {e}"
            ))
        })
    }
}

/// How to get a machine registered again when the hub holds a token for its ID that this
/// machine does not have, or the other way round.
pub const RECOVER: &str = "the hub already holds a token for this machine's ID. Restore \
    `machine.json` in the Ignite home (it holds the token), or delete it to register as a new \
    machine, then run `ignite install` again";

/// What the machine remembers about itself between runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub machine: MachineId,
    /// The write token the hub issued; `None` until the machine has registered.
    pub token: Option<String>,
}

/// The file that holds the [`Identity`], readable by its user only.
#[derive(Clone, Debug)]
pub struct IdentityFile {
    pub path: PathBuf,
}

impl IdentityFile {
    pub fn load(&self) -> Result<Option<Identity>> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => serde_json::from_str(&text).map(Some).map_err(|e| {
                Error::Invalid(format!(
                    "{} is not a machine identity: {e}",
                    self.path.display()
                ))
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, identity: &Identity) -> Result<()> {
        use std::io::Write;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text =
            serde_json::to_string_pretty(identity).map_err(|e| Error::Invalid(e.to_string()))?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        // Only this user may read the write token. On Windows the file inherits the user
        // profile folder's permissions, which already allow only this user.
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&self.path)?;
        file.write_all(text.as_bytes())?;
        Ok(())
    }

    /// The stored identity, or a new one that this call saves.
    pub fn load_or_create(&self) -> Result<Identity> {
        if let Some(identity) = self.load()? {
            return Ok(identity);
        }
        let identity = Identity {
            machine: new_machine_id(),
            token: None,
        };
        self.save(&identity)?;
        Ok(identity)
    }
}

/// A fresh ID such as `m-7f3a91c2`, from the operating system's randomness through the standard
/// library's hasher seed.
fn new_machine_id() -> MachineId {
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u32(std::process::id());
    let id = format!("m-{:08x}", hasher.finish() as u32);
    MachineId::new(id).expect("a hex ID is a valid machine ID")
}

/// How the running daemon relates to the hub's version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VersionAction {
    /// The versions match.
    Current,
    /// The hub is on another version and commands are still running: finish them, and ask for no
    /// new work.
    Drain,
    /// The hub is on another version and nothing is running: update and reconnect now.
    UpdateNow,
}

impl VersionAction {
    /// Whether the daemon may ask the hub for work.
    pub fn may_ask_for_work(self) -> bool {
        self == VersionAction::Current
    }
}

/// Compares the daemon's version with the hub's, as the hub reports it in a heartbeat reply.
pub fn version_action(own: &Commit, hub_version: &str, running_commands: usize) -> VersionAction {
    if own.as_str() == hub_version {
        VersionAction::Current
    } else if running_commands > 0 {
        VersionAction::Drain
    } else {
        VersionAction::UpdateNow
    }
}

/// Replaces the running daemon with the executable of a newer version.
pub trait Restart {
    /// Starts `executable args` in place of this process. Returns only if it could not.
    fn restart(&self, executable: &Path, args: &[String]) -> io::Result<()>;
}

/// Restarts with `exec`: the process keeps its ID, so the login service sees no exit, and
/// supervisors it started stay its children.
///
/// Windows has no `exec`, so there the new daemon starts as a process of its own, apart from
/// this one's console, and this process exits. Supervisors keep running, as they do on macOS.
/// The new daemon may reach the machine's daemon lock before this process has exited and freed
/// it, so it is told this process's ID in `IGNITE_RESTART_FROM` and waits for the lock (see
/// `crate::machine_lock`). This process exits only once the new one has logged its start, or
/// after `HANDOVER_WAIT`: a client that finds no pipe treats it as a daemon starting while a
/// start was logged a moment ago, so no client sees a gap in which no daemon seems to run.
pub struct ExecRestart;

/// How long a restart on Windows waits for the new daemon to log its start before it exits.
#[cfg(windows)]
const HANDOVER_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

impl Restart for ExecRestart {
    #[cfg(unix)]
    fn restart(&self, executable: &Path, args: &[String]) -> io::Result<()> {
        use std::os::unix::process::CommandExt;
        Err(Command::new(executable).args(args).exec())
    }

    #[cfg(windows)]
    fn restart(&self, executable: &Path, args: &[String]) -> io::Result<()> {
        use ignite_os::proc::Processes;
        let mut command = Command::new(executable);
        Native.detach(&mut command);
        command.env(
            crate::machine_lock::RESTART_FROM,
            std::process::id().to_string(),
        );
        let child = command.args(args).spawn()?;
        let log = start_log(&Home::from_env());
        let since = std::time::Instant::now();
        while since.elapsed() < HANDOVER_WAIT
            && !log.read().iter().any(|start| start.pid == child.id())
        {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        std::process::exit(0)
    }
}

/// What a check for a newer release found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateCheck {
    /// `master` has not moved.
    UpToDate,
    /// `master` moved but nothing under `ignite/` changed, so the executable stays.
    NotNeeded { tip: Commit },
    /// The release branch's tip is not a descendant of the running commit: the daemon was
    /// installed by hand from a newer or diverged commit. Going to the tip would be a
    /// downgrade, so the executable stays until the branch moves past it.
    Ahead { tip: Commit },
    /// The executable of `to` is built and stored, and is what a restart starts.
    Ready {
        from: Commit,
        to: Commit,
        executable: PathBuf,
    },
}

/// Builds and stores new releases.
pub struct Updater<'a> {
    pub git: &'a dyn Git,
    pub builder: &'a dyn Builder,
    pub store: &'a ExecutableStore,
    /// The branch releases come from.
    pub master: &'a str,
    /// Where Ignite's own source lives in the repository.
    pub source: &'a str,
}

/// What a check for a newer release decided, before anything is built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    /// Nothing to build: the check's answer is final (never [`UpdateCheck::Ready`]).
    Done(UpdateCheck),
    /// The executable of this commit should run; it may be stored already.
    Build(Commit),
}

impl Updater<'_> {
    /// Decides whether `master` moved and files under the source directory changed, without
    /// building anything.
    pub fn plan(&self, current: &Commit) -> Result<Plan> {
        let tip = self.git.tip(self.master)?;
        if &tip == current || self.store.is_rejected(&tip) {
            return Ok(Plan::Done(UpdateCheck::UpToDate));
        }
        if !self.git.is_ancestor(current, &tip)? {
            return Ok(Plan::Done(UpdateCheck::Ahead { tip }));
        }
        let prefix = format!("{}/", self.source.trim_end_matches('/'));
        let changed = self.git.changed(current, &tip)?;
        if !changed.iter().any(|path| path.starts_with(&prefix)) {
            return Ok(Plan::Done(UpdateCheck::NotNeeded { tip }));
        }
        Ok(Plan::Build(tip))
    }

    /// Builds the new executable if `master` moved and files under the source directory changed.
    pub fn check(&self, current: &Commit) -> Result<UpdateCheck> {
        match self.plan(current)? {
            Plan::Done(check) => Ok(check),
            Plan::Build(tip) => self.prepare(current, &tip),
        }
    }

    /// Builds and stores `target`, the version the hub runs, unless it is stored already.
    pub fn prepare(&self, current: &Commit, target: &Commit) -> Result<UpdateCheck> {
        if self.store.is_rejected(target) {
            // A release that failed to start is not tried again.
            return Ok(UpdateCheck::UpToDate);
        }
        if !self.store.has(target) {
            let built = self.builder.build(target)?;
            self.store.store(target, &built)?;
        }
        self.activate(current, target)
    }

    /// Stores the executable `built` for `target`, which a build run elsewhere made, and
    /// activates it.
    pub fn install(&self, current: &Commit, target: &Commit, built: &Path) -> Result<UpdateCheck> {
        if self.store.is_rejected(target) {
            return Ok(UpdateCheck::UpToDate);
        }
        self.store.store(target, built)?;
        self.activate(current, target)
    }

    /// Points `bin/current` at the stored build of `target`, which a restart starts, and prunes
    /// the builds that are no longer needed.
    pub fn activate(&self, current: &Commit, target: &Commit) -> Result<UpdateCheck> {
        self.store.point_current_at(target)?;
        self.store.prune(&[target, current]);
        Ok(UpdateCheck::Ready {
            from: current.clone(),
            to: target.clone(),
            executable: self.store.path(target),
        })
    }
}

/// Restarts the daemon into a prepared release. Does not return on success.
pub fn restart_into(restart: &dyn Restart, executable: &Path) -> Error {
    let args: Vec<String> = DAEMON_ARGS.iter().map(|s| (*s).to_owned()).collect();
    match restart.restart(executable, &args) {
        Ok(()) => Error::Io("restarted".to_owned()),
        Err(e) => Error::Io(format!("cannot start {}: {e}", executable.display())),
    }
}

#[cfg(test)]
mod tests;
