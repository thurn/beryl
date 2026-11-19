//! Verb processes: one long verb, such as `land`, run from start to finish in a process of its
//! own, `ignite internal verb <handle>`, which outlives restarts of the daemon.
//!
//! The daemon gives the verb its handle and records the request that started it in the
//! handle's row in `daemon.db` (see [`crate::handles`]), then starts the verb process from its
//! own executable. The verb process:
//!
//! 1. holds `verbs/<handle>/lock` in the Ignite home exclusively for its whole life, as a
//!    supervisor holds its run's `lock`, so a daemon that finds the lock free knows the process
//!    has ended, whatever its process ID now names ([`hold`], [`alive`]);
//! 2. reads the verb and the request from the handle's row, and records its own process ID,
//!    the commit of its executable and the host protocol version it speaks in the row;
//! 3. sends `host-hello`, and builds the verb's [`Context`] as the daemon would (see
//!    `main/assemble.rs`), except that commands, builds and landing trees are asked of the
//!    daemon through the host protocol (`verbproc/remote.rs`, `ignite_core::protocol::host`);
//! 4. runs the verb on an activity of its own (see [`crate::steps`]), whose steps and waits a
//!    thread sends to the daemon, which also tells it when someone asks the verb to stop
//!    (`verbproc/mirror.rs`);
//! 5. writes the verb's reply into the handle's row (`done`, or `stopped`) in one transaction,
//!    sends `host-finished` so the daemon wakes whoever waits on the handle, and exits.
//!
//! Every call that fails because the daemon cannot be reached, as while it restarts into a new
//! build, is sent again with the same body (so with the same request ID) for up to five minutes,
//! after `host-hello` and the verb's current step (`verbproc/link.rs`). The daemon starts one
//! command and makes one build per request ID, so a call repeated across a restart is answered
//! as if the first attempt had been. Calls are sent from the Ignite home, outside every build
//! tree, so the daemon does not count them as a session's activity in a tree.
//!
//! A panic in the verb writes a crash record (see [`crate::crash`]) and a failed reply.

#[cfg(test)]
mod daemon_tests;
mod link;
mod mirror;
mod remote;
#[cfg(test)]
mod tests;

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::Connection;

use ignite_core::host::HostClient;
use ignite_core::ids::{Handle, TicketId};
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::protocol::host::{Hello, PROTOCOL_VERSION, RequestIds};
use ignite_core::traits::{Clock, Error, Result};
use ignite_store::local::{handles as handle_store, trees as tree_store};

use crate::handles::lock;
use crate::server::{Context, Verb, stopped_reply};
use crate::steps::{self, Kind, Work};

pub use link::{Link, SocketHost};
pub use mirror::HostStop;
pub use remote::{HasBuilt, Remote, RemoteBuilds, RemoteLandTrees, RemoteLauncher};

/// How long a verb process keeps trying an unreachable daemon, and how often it asks.
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    /// How long a call is sent again while the daemon cannot be reached.
    pub retry_for: Duration,
    /// The pause before each repeat.
    pub pause: Duration,
    /// How often the daemon is asked whether a stop was asked, when no step changes.
    pub poll: Duration,
}

impl Default for Timing {
    fn default() -> Timing {
        Timing {
            retry_for: Duration::from_mins(5),
            pause: Duration::from_secs(1),
            poll: Duration::from_secs(1),
        }
    }
}

/// The environment variable in which the daemon names the machine, as it found it in
/// `ignite.toml`, to the verb processes it starts, so both read the same `[machine.<name>]`.
pub const HOSTNAME_VARIABLE: &str = "IGNITE_HOSTNAME";

/// The folder of `handle`'s verb process in the Ignite home `home`.
pub fn folder(home: &Path, handle: &Handle) -> PathBuf {
    home.join("verbs").join(handle.as_str())
}

/// The lock a verb process holds for its whole life, open and locked.
pub struct VerbLock {
    /// Held, never read: the open file keeps the lock.
    _file: File,
}

/// How many times [`hold`] tries a lock that someone holds, and the pause between tries: two
/// seconds in all.
const HOLD_TRIES: u32 = 200;
const HOLD_PAUSE: Duration = Duration::from_millis(10);

/// Takes `handle`'s lock in `home`, creating its folder. `None` when another process holds it.
///
/// The daemon asks whether a verb process runs by taking the same lock shared for a moment
/// ([`alive`]), which it does every few milliseconds while it waits for a new process to take
/// the lock. A new process whose first try falls in such a moment would otherwise find the lock
/// held, exit saying another process runs the handle, and leave its verb to be run again from
/// the start. So a held lock is tried for [`HOLD_TRIES`] tries: a probe lets go within
/// microseconds, while a verb process holds the lock for its whole life.
pub fn hold(home: &Path, handle: &Handle) -> io::Result<Option<VerbLock>> {
    hold_trying(home, handle, HOLD_TRIES, &mut || {
        std::thread::sleep(HOLD_PAUSE);
    })
}

/// [`hold`], trying a held lock `tries` times with `pause` between tries.
fn hold_trying(
    home: &Path,
    handle: &Handle,
    tries: u32,
    pause: &mut dyn FnMut(),
) -> io::Result<Option<VerbLock>> {
    let dir = folder(home, handle);
    std::fs::create_dir_all(&dir)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("lock"))?;
    for attempt in 1..=tries.max(1) {
        match file.try_lock() {
            Ok(()) => return Ok(Some(VerbLock { _file: file })),
            Err(TryLockError::WouldBlock) if attempt < tries => pause(),
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Error(error)) => return Err(error),
        }
    }
    Ok(None)
}

/// Whether a verb process runs `handle`: some process holds its lock. A lock file that does
/// not exist is free.
pub fn alive(home: &Path, handle: &Handle) -> bool {
    let Ok(file) = File::open(folder(home, handle).join("lock")) else {
        return false;
    };
    matches!(file.try_lock_shared(), Err(TryLockError::WouldBlock))
}

/// What a verb process runs: the verb and the context it runs with.
pub struct Assembled {
    pub verb: Arc<dyn Verb>,
    pub ctx: Context,
}

/// How a verb process ended.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// The verb's reply is in the handle's row.
    Finished(Reply),
    /// Another process holds the handle's lock.
    AlreadyRunning,
    /// The handle is unknown, has finished, or has no request recorded.
    NotRunning,
    /// The handle's lock or row could not be read.
    Failed(String),
}

/// One verb process, with what it is built from. Tests give it fakes.
pub struct Process {
    pub handle: Handle,
    /// The Ignite home.
    pub home: PathBuf,
    /// The daemon's local database.
    pub db: Arc<Mutex<Connection>>,
    pub host: Arc<dyn HostClient>,
    /// The Ignite commit of this executable.
    pub executable: String,
    pub clock: Arc<dyn Clock>,
    pub timing: Timing,
}

impl Process {
    /// Runs the handle's verb to the end (see the module documentation). `assemble` builds the
    /// verb named and its context from the remote parts, once the request is known.
    pub fn run(
        self,
        assemble: impl FnOnce(&Arc<Remote>, &str, &Request) -> Result<Assembled>,
    ) -> Outcome {
        let _lock = match hold(&self.home, &self.handle) {
            Ok(Some(lock)) => lock,
            Ok(None) => return Outcome::AlreadyRunning,
            Err(error) => return Outcome::Failed(format!("the lock of {}: {error}", self.handle)),
        };
        let found = crate::handles::running_request(&lock(&self.db), &self.handle);
        let (verb_name, request) = match found {
            Ok(Some(found)) => found,
            Ok(None) => return Outcome::NotRunning,
            Err(error) => return Outcome::Failed(format!("the row of {}: {error}", self.handle)),
        };
        let pid = std::process::id();
        let process = handle_store::Process {
            pid: i64::from(pid),
            executable: self.executable.clone(),
            protocol: i64::from(PROTOCOL_VERSION),
        };
        if let Err(error) =
            handle_store::set_process(&lock(&self.db), self.handle.as_str(), &process)
        {
            eprintln!(
                "ignite: cannot record the verb process of {}: {error}",
                self.handle
            );
        }
        let link = Arc::new(Link::new(
            self.host.clone(),
            Hello {
                handle: self.handle.clone(),
                protocol: PROTOCOL_VERSION,
                executable: self.executable.clone(),
                pid,
            },
            self.timing,
        ));
        let stop_first = match link.hello() {
            Ok(reply) => reply.stop || !reply.running,
            Err(error) => {
                // A verb that needs nothing from the daemon can still finish.
                eprintln!("ignite: cannot reach the daemon: {error}");
                false
            }
        };
        let (reply, stopped) = if stop_first {
            (stopped_reply(&verb_name, Some(&self.handle)), true)
        } else {
            let remote = Arc::new(Remote {
                link: link.clone(),
                ids: Arc::new(RequestIds::new(self.handle.clone())),
                clock: self.clock.clone(),
                notes: folder(&self.home, &self.handle),
            });
            match assemble(&remote, &verb_name, &request) {
                Ok(assembled) => self.run_verb(&link, &verb_name, &request, assembled),
                Err(error) => {
                    let text = format!("failed   `ignite {verb_name}` could not start: {error}");
                    (Reply::text(Exit::Usage, text), false)
                }
            }
        };
        self.finish(&link, reply, stopped)
    }

    /// Runs the verb on an activity mirrored to the daemon and returns its reply, or the reply
    /// of a stopped verb when it was asked to stop, with whether it was.
    fn run_verb(
        &self,
        link: &Arc<Link>,
        name: &str,
        req: &Request,
        assembled: Assembled,
    ) -> (Reply, bool) {
        let Assembled { verb, ctx } = assembled;
        let activity = steps::global().begin(Kind::Verb(name.to_owned()), self.work(&ctx));
        activity.attach_handle(&self.handle);
        activity.keep_stop_with(Arc::new(HostStop(link.clone())));
        link.attach(&activity);
        let mirror = mirror::Mirror::start(link.clone(), activity.clone(), self.timing);
        let entered = activity.enter();
        let reply = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| verb.run(&ctx, req)))
            .unwrap_or_else(|_| {
                Reply::text(
                    Exit::Usage,
                    format!("failed   `{name}` panicked in its verb process"),
                )
            });
        drop(entered);
        mirror.stop();
        let stopped = activity.stop_requested();
        let reply = if stopped {
            stopped_reply(name, Some(&self.handle))
        } else {
            reply
        };
        activity.finish();
        (reply, stopped)
    }

    /// Whom the verb works for, as the server records it for a verb on one of its threads.
    fn work(&self, ctx: &Context) -> Work {
        let ticket = ctx.tree.as_ref().and_then(|tree| {
            let row = tree_store::get(&lock(&self.db), tree.as_str()).ok()??;
            (row.state != tree_store::State::Free.as_str())
                .then_some(row.ticket)?
                .and_then(|ticket| TicketId::new(ticket).ok())
        });
        Work {
            class: None,
            ticket,
            session: ctx.session.clone(),
            tree: ctx.tree.clone(),
            tree_path: ctx.tree.as_ref().map(|tree| {
                ctx.repo
                    .join(&ctx.config.repository.trees)
                    .join(tree.as_str())
            }),
        }
    }

    /// Writes the reply into the handle's row and tells the daemon.
    fn finish(&self, link: &Link, reply: Reply, stopped: bool) -> Outcome {
        let written = crate::handles::finish_in_row(
            &lock(&self.db),
            &self.handle,
            &reply,
            stopped,
            self.clock.now(),
        );
        match written {
            Ok(true) => {}
            Ok(false) => eprintln!(
                "ignite: {} was no longer running when its verb finished; its reply is dropped",
                self.handle
            ),
            Err(error) => {
                return Outcome::Failed(format!(
                    "cannot write the reply of {}: {error}",
                    self.handle
                ));
            }
        }
        if let Err(error) = link.finished() {
            // The daemon notices the finished row on its own within a second of coming back.
            eprintln!(
                "ignite: cannot tell the daemon that {} finished: {error}",
                self.handle
            );
        }
        Outcome::Finished(reply)
    }
}

/// The hidden `ignite internal verb <handle>` subcommand: runs the handle's verb in this process
/// for the main checkout of the repository at `cwd`, or the one `ignite install` recorded.
/// Returns the exit code: 0 once the reply is in the handle's row, else 1.
pub fn run(handle: &str, cwd: &Path) -> i32 {
    let Ok(handle) = Handle::new(handle) else {
        eprintln!("ignite: {handle} is not a handle");
        return 1;
    };
    let home = crate::update::Home::from_env();
    let ignite = std::env::current_exe()
        .and_then(|exe| exe.canonicalize())
        .unwrap_or_default();
    let version = crate::update::ExecutableStore::version_of(&ignite)
        .map_or_else(|| "unknown".to_owned(), |commit| commit.to_string());
    crate::crash::install_hook(home.root.clone(), version.clone());
    let db = match ignite_store::local::open(&home.root.join("daemon.db")) {
        Ok(db) => Arc::new(Mutex::new(db)),
        Err(error) => {
            eprintln!("ignite: cannot open daemon.db: {error}");
            return 1;
        }
    };
    let options = crate::main::DaemonOptions {
        repo: crate::main::checkout(cwd, &home),
        home: home.clone(),
        hostname: std::env::var(HOSTNAME_VARIABLE)
            .unwrap_or_else(|_| crate::doctor::Host::hostname(&crate::doctor::NativeHost)),
        ignite,
        version: version.clone(),
        feed_wait_s: 60,
    };
    let process = Process {
        handle,
        home: home.root.clone(),
        db: db.clone(),
        host: Arc::new(SocketHost {
            socket: crate::server::default_socket()
                .to_string_lossy()
                .into_owned(),
            cwd: home.root.clone(),
        }),
        executable: version,
        clock: Arc::new(ignite_core::time::SystemClock),
        timing: Timing::default(),
    };
    let outcome = process.run(|remote, verb, req| assemble(options, &db, remote, verb, req));
    match outcome {
        Outcome::Finished(_) => 0,
        Outcome::AlreadyRunning => {
            eprintln!("ignite: another verb process already runs this handle");
            1
        }
        Outcome::NotRunning => {
            eprintln!("ignite: the handle is not running; nothing to do");
            0
        }
        Outcome::Failed(reason) => {
            eprintln!("ignite: {reason}");
            1
        }
    }
}

/// The verb `name` and its context, built as the daemon builds its own (see
/// `main/assemble.rs`), with the remote parts.
fn assemble(
    options: crate::main::DaemonOptions,
    db: &Arc<Mutex<Connection>>,
    remote: &Arc<Remote>,
    name: &str,
    req: &Request,
) -> Result<Assembled> {
    let base = crate::main::assemble::Base::read(options)?;
    let builds = Arc::new(RemoteBuilds {
        remote: remote.clone(),
        has_built: has_built(&base, db.clone()),
    });
    let launcher = Arc::new(RemoteLauncher(remote.clone()));
    let landing = {
        let remote = remote.clone();
        move |local| -> Arc<dyn crate::land::env::LandTrees> {
            Arc::new(RemoteLandTrees { remote, local })
        }
    };
    let idle_host = {
        let remote = remote.clone();
        move |local| -> Box<dyn crate::idle::IdleHost> {
            Box::new(crate::idle::RemoteIdleHost {
                remote,
                local: Box::new(local),
            })
        }
    };
    let (mut ctx, verbs) = crate::main::assemble::verb_side(
        &base,
        launcher,
        builds,
        landing,
        Arc::new(remote::RemoteLender(remote.clone())),
        idle_host,
    )?;
    let verb = verbs
        .into_iter()
        .map(|(verb, _)| verb)
        .find(|verb| verb.name() == name)
        .ok_or_else(|| Error::Invalid(format!("`{name}` does not run in a verb process")))?;
    ctx.session.clone_from(&req.session);
    ctx.tree = ctx.tree_of(&req.cwd);
    Ok(Assembled {
        verb: Arc::from(verb),
        ctx,
    })
}

/// Whether a tree has built a product, answered as the daemon's build manager answers it: the
/// daemon recorded the tree holding it (for any packages), or every directory the product
/// writes is in the tree and not empty.
fn has_built(base: &crate::main::assemble::Base, db: Arc<Mutex<Connection>>) -> HasBuilt {
    let manifest = base.manifest.clone();
    let trees = base.trees_dir.clone();
    Arc::new(move |tree, product| {
        let held = ignite_store::local::builds::all(&lock(&db)).is_ok_and(|rows| {
            rows.iter().any(|row| {
                row.tree == tree.as_str()
                    && row.product == product
                    && row.outcome == ignite_store::local::builds::HELD
            })
        });
        held || manifest.products.get(product).is_some_and(|declared| {
            !declared.outputs.is_empty()
                && declared.outputs.iter().all(|output| {
                    std::fs::read_dir(trees.join(tree.as_str()).join(output))
                        .is_ok_and(|mut entries| entries.next().is_some())
                })
        })
    })
}
