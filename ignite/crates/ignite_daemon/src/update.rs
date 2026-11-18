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
use std::fmt::Write as _;
use std::hash::{BuildHasher, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use ignite_core::config::{Config, Os};
use ignite_core::ids::{Commit, MachineId};
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::protocol::hub::{MachineRegister, MachineRegistered};
use ignite_core::traits::{Error, Git, Result};
use ignite_os::Native;
use ignite_os::service::Service;
use serde::{Deserialize, Serialize};

use crate::doctor::{Host, NativeHost};
use crate::server::{Context, Verb};

mod rollback;

pub use rollback::{CRASH_WINDOW_S, HEALTHY_AFTER_S, KEEP_EXTRA, Startup};

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
        self.begin_trial(commit);
        self.flip(commit)
    }

    fn flip(&self, commit: &Commit) -> io::Result<()> {
        let partial = self
            .root
            .join(format!("current.partial-{}", std::process::id()));
        // A partial file left by a crash would make `symlink` fail.
        let _ = std::fs::remove_file(&partial);
        #[cfg(unix)]
        std::os::unix::fs::symlink(self.path(commit), &partial)?;
        #[cfg(windows)]
        {
            let launcher = self.current_link();
            if std::fs::read_to_string(&launcher).ok().as_deref() != Some(CURRENT_CMD) {
                std::fs::write(&launcher, CURRENT_CMD)?;
            }
            std::fs::write(&partial, self.path(commit).display().to_string())?;
        }
        std::fs::rename(&partial, self.root.join("current"))
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

/// The directory that `cargo` is found in when the service's minimal `PATH` lacks it.
fn cargo_command() -> Command {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let cargo = home
        .map(|h| h.join(".cargo").join("bin").join("cargo"))
        .filter(|p| p.is_file());
    Command::new(cargo.unwrap_or_else(|| PathBuf::from("cargo")))
}

/// Runs `cargo build --release` for the `ignite` executable in the workspace at `tree/ignite`.
fn cargo_build(tree: &Path) -> Result<PathBuf> {
    let workspace = tree.join("ignite");
    let target = workspace.join("target");
    let output = cargo_command()
        .args([
            "build",
            "--release",
            "--package",
            "ignite_cli",
            "--manifest-path",
        ])
        .arg(workspace.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&target)
        .output()
        .map_err(|e| Error::Io(format!("cannot run cargo: {e}")))?;
    if !output.status.success() {
        let text = String::from_utf8_lossy(&output.stderr);
        let tail: Vec<&str> = text.lines().rev().take(15).collect();
        let tail: Vec<&str> = tail.into_iter().rev().collect();
        return Err(Error::Io(format!(
            "cargo build failed in {}:\n{}",
            workspace.display(),
            tail.join("\n")
        )));
    }
    Ok(target.join("release").join("ignite"))
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

/// Builds each release in one tree of its own, reset to the commit being built, so that
/// the next update starts from the last one's compiled output and never touches a build tree.
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
}

impl Builder for TreeBuilder {
    fn build(&self, commit: &Commit) -> Result<PathBuf> {
        let tree = self.tree();
        if tree.join(".git").exists() {
            let keep: Vec<String> = Self::KEEP.iter().map(|s| (*s).to_owned()).collect();
            ignite_git::worktree::reset(&tree, Self::BRANCH, commit, &keep)?;
        } else {
            std::fs::create_dir_all(&self.work)?;
            ignite_git::worktree::add(&self.repo, &tree, Self::BRANCH, commit)?;
        }
        cargo_build(&tree)
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

/// Everything `ignite install` touches.
pub struct InstallDeps<'a> {
    pub host: &'a dyn Host,
    pub builder: &'a dyn Builder,
    pub store: &'a ExecutableStore,
    pub identity: &'a IdentityFile,
    pub registrar: &'a dyn Registrar,
    pub service: &'a dyn Service,
}

/// What a finished install did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    pub machine: MachineId,
    pub version: Commit,
    pub executable: PathBuf,
}

/// Builds the executable of the checkout at `head`, stores it, registers the machine with the hub
/// and installs the login service. The service goes last, so a failed install leaves no daemon.
pub fn install(deps: &InstallDeps, config: &Config, head: &Commit) -> Result<Installed> {
    let name = deps.host.hostname();
    let declared = config.machine(&name).ok_or_else(|| {
        Error::Invalid(format!(
            "this machine is called {name:?} but ignite.toml declares no [machine.{name}]; \
             add one and run `ignite install` again"
        ))
    })?;
    let built = deps.builder.build(head)?;
    let executable = deps.store.store(head, &built)?;
    deps.store.point_current_at(head)?;

    let mut identity = deps.identity.load_or_create()?;
    let known_token = identity.token.clone();
    let registered = deps.registrar.register(
        &MachineRegister {
            machine: identity.machine.clone(),
            name,
            os: declared.os.or_else(Os::current).ok_or_else(|| {
                Error::Invalid("this operating system is not supported".to_owned())
            })?,
            cores: deps.host.cores(),
            memory_mb: deps.host.memory_mb(),
            families: declared.families.clone().unwrap_or_default(),
            version: head.as_str().to_owned(),
        },
        known_token.as_deref(),
    )?;
    identity.token = Some(registered.token);
    deps.identity.save(&identity)?;

    let args: Vec<String> = DAEMON_ARGS.iter().map(|s| (*s).to_owned()).collect();
    deps.service.install(&deps.store.current_link(), &args)?;
    Ok(Installed {
        machine: identity.machine,
        version: head.clone(),
        executable,
    })
}

/// How an exit code and a message answer a failed operation.
fn failure(error: &Error) -> Reply {
    let exit = match error {
        Error::Unreachable(_) => Exit::Unreachable,
        Error::Conflict(_) => Exit::Conflict,
        _ => Exit::Usage,
    };
    Reply::text(exit, error.to_string())
}

/// `ignite install`: builds the executable, registers the machine and starts the daemon at login.
pub struct InstallVerb;

impl Verb for InstallVerb {
    fn name(&self) -> &'static str {
        "install"
    }

    fn run(&self, ctx: &Context, _req: &Request) -> Reply {
        install_checkout(&ctx.repo, &ctx.config)
    }
}

/// `ignite install` for the checkout at `repo`, run by the client itself, since no daemon runs
/// before the first install. It also records the checkout in the home folder, where the daemon
/// that the login service starts finds it.
pub fn install_checkout(repo: &Path, config: &Config) -> Reply {
    let home = Home::from_env();
    let head = match ignite_git::refs::head(repo) {
        Ok(head) => head,
        Err(error) => return failure(&error),
    };
    let recorded = home
        .create()
        .and_then(|()| std::fs::write(home.checkout_file(), repo.to_string_lossy().as_bytes()));
    if let Err(error) = recorded {
        return failure(&Error::from(error));
    }
    let store = home.store();
    let identity = home.identity();
    let deps = InstallDeps {
        host: &NativeHost,
        builder: &CheckoutBuilder {
            repo: repo.to_path_buf(),
        },
        store: &store,
        identity: &identity,
        registrar: &HttpRegistrar {
            url: config.hub.url.clone(),
            project: config.hub.project.clone(),
        },
        service: &Native,
    };
    match install(&deps, config, &head) {
        Ok(done) => {
            let mut text = String::new();
            let _ = writeln!(text, "installed  {}", done.version);
            let _ = writeln!(text, "machine    {}", done.machine);
            let _ = writeln!(text, "binary     {}", done.executable.display());
            let _ = write!(
                text,
                "daemon     starts at login; `ignite doctor` checks it"
            );
            Reply::text(Exit::Success, text)
        }
        Err(error) => failure(&error),
    }
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
/// The new daemon may start before this process has exited and freed the machine's daemon
/// lock.
pub struct ExecRestart;

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
        command.args(args).spawn()?;
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

impl Updater<'_> {
    /// Builds the new executable if `master` moved and files under the source directory changed.
    pub fn check(&self, current: &Commit) -> Result<UpdateCheck> {
        let tip = self.git.tip(self.master)?;
        if &tip == current {
            return Ok(UpdateCheck::UpToDate);
        }
        let prefix = format!("{}/", self.source.trim_end_matches('/'));
        let changed = self.git.changed(current, &tip)?;
        if !changed.iter().any(|path| path.starts_with(&prefix)) {
            return Ok(UpdateCheck::NotNeeded { tip });
        }
        self.prepare(current, &tip)
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
