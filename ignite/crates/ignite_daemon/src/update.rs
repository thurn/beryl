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

use ignite_core::ids::{Commit, MachineId};
use ignite_core::protocol::hub::{MachineRegister, MachineRegistered};
use ignite_core::traits::{Error, Git, Result};
#[cfg(windows)]
use ignite_os::Native;
use serde::{Deserialize, Serialize};

mod build;
pub mod direct;
mod install;
mod lastgood;
pub mod prebuild;
mod rollback;
pub mod scope;
pub mod selftest;
mod starts;

pub use build::{CheckoutBuilder, RELEASE_BUILD_HANG_LIMIT_S, ReleaseBuilder, TreeBuilder};
pub use install::{
    BUILT_VARIABLE, Handoff, HubFallback, InstallDeps, InstallVerb, Installed, LocalHub, Outcome,
    PathLink, ProcessHandoff, ServedHub, finish_install, install, install_checkout, link_onto_path,
    main_checkout,
};
pub use lastgood::{CRASH_STARTS, CrashLoop, LAST_GOOD_AFTER_S, detect_crash_loop, roll_back};
pub use rollback::{CRASH_WINDOW_S, HEALTHY_AFTER_S, IN_USE, KEEP_EXTRA, Startup, hold_release};
pub(crate) use starts::home_of;
pub use starts::{
    StartRecord, forget_start, guard_start, record_adopted_from, record_clean_stop,
    record_clean_stop_from, spawn_health_marks, start_log, started_at,
};

/// The argument the monitor's login service entry starts the executable with.
pub const MONITOR_VERB: &str = "monitor";

/// The directory of Ignite's own source in the repository; a change under it means a new build,
/// and a landing that touches it may move the release ref.
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
    /// `$IGNITE_HOME`, or `~/.config/ignite` (see [`ignite_core::config::home_dir`]).
    pub fn from_env() -> Home {
        let root = std::env::var_os("IGNITE_HOME").map_or_else(
            || {
                ignite_core::config::home_dir(|name| std::env::var_os(name))
                    .unwrap_or_default()
                    .join(".config")
                    .join("ignite")
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

    /// The folder that holds the update tree and its build logs (see [`TreeBuilder`]).
    pub fn update_dir(&self) -> PathBuf {
        self.root.join("update")
    }

    /// The update tree, where the daemon builds its own releases. Its build directory is kept
    /// between updates, so `doctor` does not report it as a stray build.
    pub fn update_tree(&self) -> PathBuf {
        self.update_dir().join(TreeBuilder::TREE)
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
            std::fs::create_dir_all(parent).map_err(|e| with_path(parent, &e))?;
        }
        match std::fs::DirBuilder::new().mode(0o700).create(dir) {
            Err(error) if error.kind() != io::ErrorKind::AlreadyExists => {
                return Err(with_path(dir, &error));
            }
            _ => {}
        }
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| with_path(dir, &e))
    }
    #[cfg(windows)]
    {
        // The folder inherits the user profile's permissions, which allow only this user.
        std::fs::create_dir_all(dir).map_err(|e| with_path(dir, &e))
    }
}

fn with_path(path: &Path, error: &io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{}: {error}", path.display()))
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

    /// The directory that holds stored builds (`<home>/bin`).
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The daemon's home, which contains the executable store.
    pub(crate) fn home_dir(&self) -> Option<&Path> {
        self.root.parent()
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
        let mut real = ignite_os::paths::canonicalize(executable).ok()?;
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

/// The last lines of `text`, for an error message.
fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().rev().take(15).collect();
    lines.into_iter().rev().collect::<Vec<_>>().join("\n")
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
            Err(e) => Err(Error::Io(format!("{}: {e}", self.path.display()))),
        }
    }

    pub fn save(&self, identity: &Identity) -> Result<()> {
        use std::io::Write;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| Error::Io(format!("{}: {e}", dir.display())))?;
        }
        let text =
            serde_json::to_string_pretty(identity).map_err(|e| Error::Invalid(e.to_string()))?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        // Only this user may read the write token. On Windows the file inherits the user
        // profile folder's permissions, which already allow only this user.
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options
            .open(&self.path)
            .map_err(|e| Error::Io(format!("{}: {e}", self.path.display())))?;
        file.write_all(text.as_bytes())
            .map_err(|e| Error::Io(format!("{}: {e}", self.path.display())))?;
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
/// supervisors it started stay its children. The new image keeps only standard input, output
/// and error (see `ignite_os::proc::replace_image`), so a client connection that another thread
/// had just accepted is closed rather than carried into a daemon that never answers it.
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
        Err(ignite_os::proc::replace_image(executable, args))
    }

    #[cfg(windows)]
    fn restart(&self, executable: &Path, args: &[String]) -> io::Result<()> {
        use std::process::Command;

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
    /// The release ref's tip lacks changes to Ignite's source that the running commit has:
    /// the daemon was installed by hand from a newer or diverged commit. Going to the tip would
    /// be a downgrade, so the executable stays until the branch moves past it. A tip that holds
    /// the running commit's changes as other commits, as a rebased copy does, is not ahead.
    Ahead { tip: Commit },
    /// The release ref's tip was rolled back or failed its selftest on this machine, so the
    /// executable stays until the branch moves again.
    Rejected { tip: Commit },
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
        if &tip == current {
            return Ok(Plan::Done(UpdateCheck::UpToDate));
        }
        if self.store.is_rejected(&tip) {
            return Ok(Plan::Done(UpdateCheck::Rejected { tip }));
        }
        // A tip that does not contain the running commit may still hold its changes: a commit
        // pushed to the release ref by hand and then landed is rebased, and the landing moves
        // the release ref to the rebased copy. Staying on the hand-pushed commit then would
        // refuse every later release. Only a tip that lacks the running commit's changes to
        // Ignite's source would be a downgrade.
        if !self.git.is_ancestor(current, &tip)?
            && !self.git.holds_changes(&tip, current, self.source)?
        {
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
