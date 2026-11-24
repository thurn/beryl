//! `ignite install`: builds the executable of the checkout it runs in, stores it, registers the
//! machine with the hub and installs the login services.
//!
//! The steps after the build run in the executable just built, not in the one that runs
//! `ignite install`: an older `ignite` would otherwise write the login service the way its own
//! code does. [`install`] therefore hands the rest of the install to the stored executable
//! ([`Handoff`]), which runs `ignite install` again with [`BUILT_VARIABLE`] set and goes
//! straight to [`finish_install`].
//!
//! On the hub's machine the install also writes the hub's own login service, `ignite hub`
//! started from `bin/current` with the hub's listen addresses ([`hub_settings`]), and leaves
//! those addresses out of the daemon's service. Before the first install nothing serves the
//! hub there, so when the hub cannot be reached, the install serves it itself for the length of
//! the registration ([`HubFallback`], [`ServedHub`]).

use std::ffi::OsStr;
use std::fmt::Write as _;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use ignite_core::config::{Config, Os};
use ignite_core::ids::{Commit, MachineId};
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::protocol::hub::{MachineRegister, MachineRegistered};
use ignite_core::traits::{Error, Result};
use ignite_os::Native;
use ignite_os::service::{HUB_VARIABLES, Service};

use super::{
    Builder, CheckoutBuilder, DAEMON_ARGS, ExecutableStore, Home, HttpRegistrar, IdentityFile,
    MONITOR_VERB, Registrar,
};
use crate::doctor::{Host, NativeHost};
use crate::server::{Context, Verb};

/// The variable that tells `ignite install` the build is done. It holds the commit whose stored
/// executable is running the rest of the install.
pub const BUILT_VARIABLE: &str = "IGNITE_INSTALL_BUILT";

pub use ignite_os::service::HUB_LISTEN_VARIABLE;

/// The arguments the hub's login service runs the executable with.
pub const HUB_ARGS: &[&str] = &["hub"];

/// Everything `ignite install` touches.
pub struct InstallDeps<'a> {
    pub host: &'a dyn Host,
    pub builder: &'a dyn Builder,
    pub store: &'a ExecutableStore,
    pub identity: &'a IdentityFile,
    pub registrar: &'a dyn Registrar,
    pub service: &'a dyn Service,
    /// The executable running this install. When it is not the one just built, the build hands
    /// the rest of the install to the new one.
    pub running: &'a Path,
    pub handoff: &'a dyn Handoff,
    /// The install's environment, from which [`hub_settings`] reads the hub's listen addresses.
    pub environment: &'a dyn Fn(&str) -> Option<String>,
}

/// What a finished install did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    pub machine: MachineId,
    pub version: Commit,
    pub executable: PathBuf,
    /// Whether the monitor was registered; it is not on a platform without one.
    pub monitor: bool,
    /// The address the hub's login service listens on, on the hub's machine.
    pub hub: Option<String>,
}

/// How [`install`] ended.
#[derive(Debug)]
pub enum Outcome {
    /// This process did every step.
    Installed(Installed),
    /// The freshly built executable did the steps after the build, and answered with this
    /// reply.
    HandedOff(Reply),
}

/// Runs the steps after the build in another executable.
pub trait Handoff {
    /// Runs the rest of the install for `built` in `executable`, its stored build, and returns
    /// that executable's answer.
    fn finish(&self, executable: &Path, built: &Commit) -> Reply;
}

/// Runs `<executable> install` in the checkout with [`BUILT_VARIABLE`] set, and passes on what
/// it prints and its exit code.
pub struct ProcessHandoff {
    pub repo: PathBuf,
}

impl Handoff for ProcessHandoff {
    fn finish(&self, executable: &Path, built: &Commit) -> Reply {
        let output = Command::new(executable)
            .arg("install")
            .env(BUILT_VARIABLE, built.as_str())
            .current_dir(&self.repo)
            .stdin(Stdio::null())
            .output();
        let output = match output {
            Ok(output) => output,
            Err(error) => {
                return Reply::text(
                    Exit::Usage,
                    format!("cannot run {}: {error}", executable.display()),
                );
            }
        };
        let mut text = String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_owned();
        let errors = String::from_utf8_lossy(&output.stderr);
        if !errors.trim().is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(errors.trim_end());
        }
        let exit = output
            .status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .and_then(|code| Exit::try_from(code).ok())
            .unwrap_or(Exit::Usage);
        Reply::text(exit, text)
    }
}

/// Whether two paths name the same file, following links.
fn same_file(a: &Path, b: &Path) -> bool {
    match (
        ignite_os::paths::canonicalize(a),
        ignite_os::paths::canonicalize(b),
    ) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Builds the executable of the checkout at `head` and stores it. When this process runs that
/// executable, it then registers the machine and installs the login services itself
/// ([`finish_install`]); otherwise it hands those steps to the stored executable.
pub fn install(deps: &InstallDeps, config: &Config, head: &Commit) -> Result<Outcome> {
    let built = deps.builder.build(head)?;
    let executable = deps.store.store(head, &built)?;
    deps.store.point_current_at(head)?;
    if !same_file(deps.running, &executable) {
        return Ok(Outcome::HandedOff(deps.handoff.finish(&executable, head)));
    }
    finish_install(deps, config, head).map(Outcome::Installed)
}

/// The steps after the build: registers the machine with the hub and installs the login
/// services for the stored build of `head`. The services go last, so a failed install leaves
/// no daemon.
pub fn finish_install(deps: &InstallDeps, config: &Config, head: &Commit) -> Result<Installed> {
    let hostname = deps.host.hostname();
    let declared = config.machine(&hostname);
    if !deps.store.has(head) {
        return Err(Error::Invalid(format!(
            "the build of {head} is not stored; run `ignite install` again"
        )));
    }
    let mut identity = deps.identity.load_or_create()?;
    let known_token = identity.token.clone();
    let registered = deps.registrar.register(
        &MachineRegister {
            machine: identity.machine.clone(),
            name: hostname,
            os: declared
                .and_then(|machine| machine.os)
                .or_else(Os::current)
                .ok_or_else(|| {
                    Error::Invalid("this operating system is not supported".to_owned())
                })?,
            cores: deps.host.cores(),
            memory_mb: deps.host.memory_mb(),
            families: declared
                .and_then(|machine| machine.families.clone())
                .unwrap_or_default(),
            version: head.as_str().to_owned(),
        },
        known_token.as_deref(),
    )?;
    identity.token = Some(registered.token);
    deps.identity.save(&identity)?;

    let hub = hub_settings(deps.environment, deps.service);
    let args: Vec<String> = DAEMON_ARGS.iter().map(|s| (*s).to_owned()).collect();
    // The daemon's service goes first: replacing it stops a daemon that an older install left
    // serving the hub, which frees the hub's address for the hub's own service.
    deps.service.install(&deps.store.current_link(), &args)?;
    // The monitor runs the last-good build, so that it works when the newest one does not.
    let monitor = match deps
        .service
        .install_monitor(&deps.store.monitor_program(), &[MONITOR_VERB.to_owned()])
    {
        Ok(()) => true,
        Err(error) if error.kind() == io::ErrorKind::Unsupported => false,
        Err(error) => return Err(error.into()),
    };
    let listen = hub
        .iter()
        .find(|(name, _)| name == HUB_LISTEN_VARIABLE)
        .map(|(_, value)| value.clone());
    if listen.is_some() {
        let args: Vec<String> = HUB_ARGS.iter().map(|s| (*s).to_owned()).collect();
        deps.service
            .install_hub(&deps.store.current_link(), &args, &hub)?;
    }
    Ok(Installed {
        monitor,
        hub: listen,
        machine: identity.machine,
        version: head.clone(),
        executable: deps.store.path(head),
    })
}

/// The hub's listen addresses ([`HUB_VARIABLES`]) for its login service: each from the install's
/// `environment`, else from what the installed services start with (the hub's, else the
/// daemon's, which carried them before the hub had a service of its own), so that running
/// `ignite install` again, or for the first time since the hub got its own service, keeps them.
/// Empty on every machine but the hub's.
pub fn hub_settings(
    environment: &dyn Fn(&str) -> Option<String>,
    service: &dyn Service,
) -> Vec<(String, String)> {
    let settings: Vec<(String, String)> = HUB_VARIABLES
        .iter()
        .filter_map(|name| {
            let value = environment(name)
                .filter(|value| !value.is_empty())
                .or_else(|| service.recorded_variable(name).ok().flatten())?;
            Some(((*name).to_owned(), value))
        })
        .collect();
    // Without the writable listener there is no hub to serve.
    if settings.iter().any(|(name, _)| name == HUB_LISTEN_VARIABLE) {
        settings
    } else {
        Vec::new()
    }
}

/// The hub this machine serves, started for one registration.
pub trait LocalHub {
    /// Serves the hub here, registers with it and stops serving. Fails when it cannot serve,
    /// as when another process, such as a running daemon, already listens on its address.
    fn register_here(
        &self,
        request: &MachineRegister,
        token: Option<&str>,
    ) -> Result<MachineRegistered>;
}

/// Registers with the hub at its URL, and on the hub's machine, when that URL cannot be
/// reached, with a hub served by this process: there the daemon that serves the hub is the one
/// the install sets up, so before the first install nothing serves it.
pub struct HubFallback<'a> {
    pub remote: &'a dyn Registrar,
    /// The hub this machine serves; `None` on every other machine.
    pub local: Option<&'a dyn LocalHub>,
}

impl Registrar for HubFallback<'_> {
    fn register(
        &self,
        request: &MachineRegister,
        token: Option<&str>,
    ) -> Result<MachineRegistered> {
        let why = match self.remote.register(request, token) {
            Err(Error::Unreachable(why)) => why,
            other => return other,
        };
        let Some(local) = self.local else {
            return Err(Error::Unreachable(why));
        };
        local.register_here(request, token).map_err(|local_error| {
            Error::Unreachable(format!(
                "{why}; this machine serves the hub, and serving it here failed too: \
                 {local_error}"
            ))
        })
    }
}

/// The hub served on the hub's machine: the database `hub.db` in `dir`, at the address in
/// [`HUB_LISTEN_VARIABLE`].
pub struct ServedHub {
    pub config: Config,
    pub listen: String,
    pub dir: PathBuf,
}

impl LocalHub for ServedHub {
    fn register_here(
        &self,
        request: &MachineRegister,
        token: Option<&str>,
    ) -> Result<MachineRegistered> {
        std::fs::create_dir_all(&self.dir)?;
        let running = ignite_hub::server::HubServer {
            config: self.config.clone(),
            listen: self.listen.clone(),
            database: self.dir.join("hub.db"),
            wordmark: None,
        }
        .start()
        .map_err(|e| Error::Io(format!("cannot serve the hub at {}: {e}", self.listen)))?;
        let mut address = running.address();
        // An address that listens everywhere is reached through the loopback address.
        if address.ip().is_unspecified() {
            address.set_ip(match address.ip() {
                IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::LOCALHOST),
                IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::LOCALHOST),
            });
        }
        let registered = HttpRegistrar {
            url: format!("http://{address}"),
            project: self.config.hub.project.clone(),
        }
        .register(request, token);
        // Stopped before the hub's login service starts, which listens on the same address.
        running.stop();
        registered
    }
}

/// The repository's main checkout, for a checkout that may be one of its linked worktrees: the
/// first worktree git lists. `checkout` itself when git cannot say.
pub fn main_checkout(checkout: &Path) -> PathBuf {
    ignite_git::worktree::list(checkout)
        .ok()
        .and_then(|trees| trees.into_iter().next())
        .map_or_else(|| checkout.to_path_buf(), |tree| tree.path)
}

/// What [`link_onto_path`] did to put `ignite` on the search path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PathLink {
    /// Created or repointed this link to the current build.
    Linked(PathBuf),
    /// This link already pointed at the current build.
    Present(PathBuf),
    /// Nothing was linked; the text says what to do.
    Advice(String),
}

/// Puts `ignite` on the search path as `~/.local/bin/ignite`, a link to `current`, the link to
/// the newest stored build. It creates the link only when `~/.local/bin` is on `path` (the
/// value of `PATH`), and replaces only a link that points into the executable store; anything
/// else is left alone and the answer says where to link it.
pub fn link_onto_path(current: &Path, home: Option<&Path>, path: Option<&OsStr>) -> PathLink {
    let advice = |why: &str| {
        PathLink::Advice(format!(
            "{why}; to run `ignite` by name, link it from a directory on PATH to {}",
            current.display()
        ))
    };
    if cfg!(windows) {
        return advice("not linked on Windows");
    }
    let Some(home) = home else {
        return advice("HOME is not set");
    };
    let dir = home.join(".local").join("bin");
    let link = dir.join("ignite");
    if let Ok(meta) = std::fs::symlink_metadata(&link) {
        let target = std::fs::read_link(&link).ok();
        if target.as_deref() == Some(current) {
            return PathLink::Present(link);
        }
        let store = current.parent().unwrap_or(current);
        let ours = meta.file_type().is_symlink() && target.is_some_and(|t| t.starts_with(store));
        if !ours {
            return advice(&format!("{} is not Ignite's link", link.display()));
        }
    } else {
        let on_path = path.is_some_and(|p| std::env::split_paths(p).any(|d| d == dir));
        if !on_path {
            return advice(&format!("{} is not on PATH", dir.display()));
        }
    }
    match replace_link(&dir, &link, current) {
        Ok(()) => PathLink::Linked(link),
        Err(error) => advice(&format!("cannot link {}: {error}", link.display())),
    }
}

/// Points the symbolic link `link` in `dir` at `target`, replacing it in one step.
fn replace_link(dir: &Path, link: &Path, target: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let partial = dir.join(format!("ignite.partial-{}", std::process::id()));
    let _ = std::fs::remove_file(&partial);
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, &partial)?;
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(target, &partial)?;
    std::fs::rename(&partial, link)
}

/// How an exit code and a message answer a failed operation.
pub(crate) fn failure(error: &Error) -> Reply {
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
/// before the first install. It also records the repository's main checkout in the home folder,
/// where the daemon that the login service starts finds it: the daemon serves the main
/// checkout even when the install ran in a linked worktree.
pub fn install_checkout(repo: &Path, config: &Config) -> Reply {
    let home = Home::from_env();
    let built = std::env::var(BUILT_VARIABLE)
        .ok()
        .and_then(|commit| Commit::new(commit).ok());
    let head = match built
        .clone()
        .map_or_else(|| ignite_git::refs::head(repo), Ok)
    {
        Ok(head) => head,
        Err(error) => return failure(&error),
    };
    let main = main_checkout(repo);
    let recorded = home
        .create()
        .and_then(|()| std::fs::write(home.checkout_file(), main.to_string_lossy().as_bytes()));
    if let Err(error) = recorded {
        return failure(&Error::from(error));
    }
    let store = home.store();
    let identity = home.identity();
    let remote = HttpRegistrar {
        url: config.hub.url.clone(),
        project: config.hub.project.clone(),
    };
    let environment = |name: &str| std::env::var(name).ok();
    let served = hub_settings(&environment, &Native)
        .into_iter()
        .find(|(name, _)| name == HUB_LISTEN_VARIABLE)
        .map(|(_, listen)| ServedHub {
            config: config.clone(),
            listen,
            dir: home.root.join("hub"),
        });
    let registrar = HubFallback {
        remote: &remote,
        local: served.as_ref().map(|hub| hub as &dyn LocalHub),
    };
    let running = std::env::current_exe().unwrap_or_default();
    let deps = InstallDeps {
        host: &NativeHost,
        builder: &CheckoutBuilder {
            repo: repo.to_path_buf(),
        },
        store: &store,
        identity: &identity,
        registrar: &registrar,
        service: &Native,
        running: &running,
        handoff: &ProcessHandoff {
            repo: repo.to_path_buf(),
        },
        environment: &environment,
    };
    let outcome = match built {
        Some(_) => finish_install(&deps, config, &head).map(Outcome::Installed),
        None => install(&deps, config, &head),
    };
    match outcome {
        Ok(Outcome::HandedOff(reply)) => reply,
        Ok(Outcome::Installed(done)) => {
            let home_dir = ignite_core::config::home_dir(|name| std::env::var_os(name));
            let link = link_onto_path(
                &store.current_link(),
                home_dir.as_deref(),
                std::env::var_os("PATH").as_deref(),
            );
            Reply::text(Exit::Success, installed_text(&done, &main, &link))
        }
        Err(error) => failure(&error),
    }
}

/// What `ignite install` prints when it is done.
fn installed_text(done: &Installed, main: &Path, link: &PathLink) -> String {
    let mut text = String::new();
    let _ = writeln!(text, "installed  {}", done.version);
    let _ = writeln!(text, "machine    {}", done.machine);
    let _ = writeln!(text, "binary     {}", done.executable.display());
    let _ = writeln!(text, "checkout   {}", main.display());
    let _ = writeln!(
        text,
        "command    {}",
        match link {
            PathLink::Linked(path) => format!("linked {}", path.display()),
            PathLink::Present(path) => format!("{} is linked", path.display()),
            PathLink::Advice(text) => text.clone(),
        }
    );
    let _ = writeln!(
        text,
        "daemon     starts at login; `ignite doctor` checks it"
    );
    let _ = write!(
        text,
        "monitor    {}",
        if done.monitor {
            "checks the daemon every 2 minutes"
        } else {
            "not supported on this platform yet; the daemon is not watched"
        }
    );
    if let Some(listen) = &done.hub {
        let _ = write!(
            text,
            "\nhub        serves {listen} as a login service of its own (`ignite hub`)"
        );
    }
    text
}

#[cfg(test)]
#[path = "install_tests.rs"]
mod tests;
