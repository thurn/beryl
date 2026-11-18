//! The steps of [`super::Daemon::start`]: what the machine is ([`Base`]), the machinery that
//! runs commands and keeps trees, the check runner with landing and idle work on top of it, the
//! claims and the dispatcher, and the verbs.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};

use rusqlite::Connection;

use ignite_core::config::{Config, Machine, Os};
use ignite_core::ids::MachineId;
use ignite_core::manifest::Manifest;
use ignite_core::protocol::Class;
use ignite_core::protocol::hub::MachineRegister;
use ignite_core::time::SystemClock;
use ignite_core::traits::{Clock, Error, Git, Hub, Launcher, Result};
use ignite_git::GitCli;
use ignite_os::hostcpu::HostCpu;

use super::{DaemonOptions, desk, loops};
use crate::adopt::AdoptingLauncher;
use crate::build::{BuildManager, BuildSettings, DirectoryTrees};
use crate::cache::ResultCache;
use crate::checks::sources::HubHistory;
use crate::checks::verbs::{CheckVerb, TestVerb, WhyVerb};
use crate::checks::{CheckSettings, Checks};
use crate::claim::{BriefVerb, ClaimHook, Claims, CloseVerb, ReleaseVerb, StartVerb};
use crate::dispatchd::{DispatchVerb, Dispatcher, Setup, TellVerb};
use crate::doctor::{
    self, AdmissionSection, CoresSection, DoctorVerb, GitTrees, LoopsSection, NativeHost,
    StatusSection, StatusVerb, TreesSection,
};
use crate::hubclient::HubClient;
use crate::idle::{AuditVerb, Idle, IdleSettings, LiveHost, RebuildVerb};
use crate::land::env::{ChecksGate, MachineTrees};
use crate::land::records::Records;
use crate::land::{LandVerb, Landing, SyncVerb};
use crate::pool::{Pool, PoolVerb};
use crate::sched::{Limits, Scheduler};
use crate::server::{Context, Registry, Server};
use crate::supervise::{RunVerb, SupervisedLauncher};
use crate::trees::{TreeConfig, Trees};
use crate::update::InstallVerb;

/// What every part is built from: the configuration, the machine and its hub.
pub struct Base {
    pub options: DaemonOptions,
    pub config: Config,
    pub manifest: Manifest,
    pub declared: Machine,
    pub os: Os,
    pub machine: MachineId,
    pub token: String,
    pub clock: Arc<dyn Clock>,
    pub client: Arc<HubClient>,
    pub hub: Arc<dyn Hub>,
    pub git: Arc<dyn Git>,
    pub limits: Limits,
    /// The directory that holds the build trees.
    pub trees_dir: PathBuf,
}

impl Base {
    /// Reads `ignite.toml` and the manifests, finds this machine in them and registers it with
    /// the hub if it has no token yet.
    pub fn read(options: DaemonOptions) -> Result<Base> {
        let repo = options.repo.clone();
        let config = super::read_config(&repo)?;
        let manifest = super::read_manifest(&repo, &config).map_err(Error::Invalid)?;
        let declared = config
            .machine(&options.hostname)
            .ok_or_else(|| {
                let name = &options.hostname;
                Error::Invalid(format!("ignite.toml declares no [machine.{name}]"))
            })?
            .clone();
        let os = declared
            .os
            .or_else(Os::current)
            .ok_or_else(|| Error::Invalid("this operating system is not supported".into()))?;
        options.home.create()?;
        let (machine, token) = identity(&options, &config, os, declared.families.as_deref())?;
        let client = Arc::new(HubClient::new(&config.hub, &token, machine.as_str()));
        let hub: Arc<dyn Hub> = client.clone();
        let git: Arc<dyn Git> = Arc::new(GitCli {
            repo: repo.clone(),
            remote: config.repository.remote.clone(),
        });
        let limits = Limits::for_machine(&declared, ignite_os::Native.cores().max(1));
        Ok(Base {
            trees_dir: repo.join(&config.repository.trees),
            options,
            config,
            manifest,
            declared,
            os,
            machine,
            token,
            clock: Arc::new(SystemClock),
            client,
            hub,
            git,
            limits,
        })
    }

    /// A new connection to the machine's local database.
    pub fn open(&self) -> Result<Connection> {
        let path = self.options.home.root.join("daemon.db");
        ignite_store::local::open(&path).map_err(|e| Error::Io(e.to_string()))
    }

    fn home(&self, name: &str) -> PathBuf {
        self.options.home.root.join(name)
    }

    fn families(&self) -> Option<Vec<String>> {
        self.declared.families.clone()
    }

    fn trees_source(&self) -> DirectoryTrees {
        DirectoryTrees {
            root: self.trees_dir.clone(),
        }
    }
}

/// What runs commands and keeps the trees.
pub struct Machinery {
    pub scheduler: Arc<Scheduler>,
    pub launcher: Arc<dyn Launcher>,
    pub builds: Arc<BuildManager>,
    pub trees: Trees,
}

/// The scheduler, the supervisor, builds and the trees, created at the tip if missing.
pub fn machinery(base: &Base) -> Result<Machinery> {
    let scheduler = Arc::new(Scheduler::new(
        base.limits.clone(),
        base.clock.clone(),
        // One meter, read by the one sampler thread: a second reading microseconds after the
        // first would see no CPU ticks.
        Arc::new(ignite_os::hostcpu::HostMeter::new()),
    ));
    Scheduler::sample_every(&scheduler);
    let supervised = SupervisedLauncher::new(&base.options.ignite, base.home("runs"))
        .with_scheduler(scheduler.clone());
    loops::collect_unfinished(&supervised);
    // Commands that a verb cut off by the restart had running are handed back to the verb when
    // it runs again (see `crate::adopt`), except those in trees `Trees::setup` resets below.
    let reset = trees_reset_at_start(&base.open()?)?;
    let launcher: Arc<dyn Launcher> = Arc::new(AdoptingLauncher::over_supervised(
        Arc::new(supervised),
        &reset,
    ));
    let builds = Arc::new(BuildManager::new(
        BuildSettings {
            manifest: base.manifest.clone(),
            records: base.home("records"),
            cores: base.limits.max_per_command(),
            class: Class::Agent,
            ignite: base.options.ignite.clone(),
            shims: base.home("shims"),
        },
        launcher.clone(),
        base.git.clone(),
        Arc::new(base.trees_source()),
    ));
    let repo = &base.options.repo;
    let tree_config = TreeConfig::new(repo, &base.config, &base.options.hostname, &base.manifest)?;
    let trees = Trees::new(
        tree_config,
        base.open()?,
        base.clock.clone(),
        builds.clone(),
    );
    trees.setup(&base.git.tip(&base.config.repository.staging)?)?;
    Ok(Machinery {
        scheduler,
        launcher,
        builds,
        trees,
    })
}

/// The directories of the trees that `Trees::setup` resets because a daemon that stopped left
/// them in the middle of something: landing trees, and trees being built or reset.
fn trees_reset_at_start(conn: &Connection) -> Result<Vec<PathBuf>> {
    let rows = ignite_store::local::trees::all(conn).map_err(|e| Error::Io(e.to_string()))?;
    let unfinished = [
        ignite_store::local::trees::State::Prebuilding,
        ignite_store::local::trees::State::Resetting,
        ignite_store::local::trees::State::Landing,
    ];
    Ok(rows
        .into_iter()
        .filter(|row| unfinished.iter().any(|state| row.state == state.as_str()))
        .map(|row| PathBuf::from(row.path))
        .collect())
}

/// The check runner, with landing and idle work, which both run checks through it.
pub fn checking(base: &Base, trees: &Trees) -> Result<(Arc<Checks>, Arc<Landing>, Arc<Idle>)> {
    let per_command = base.limits.max_per_command();
    let checks = Arc::new(Checks::new(
        CheckSettings {
            manifest: base.manifest.clone(),
            os: base.os,
            families: base.families(),
            toolchain: toolchain(&base.options.repo),
            outputs: base.home("outputs"),
            cores: per_command,
            ignite: base.options.version.clone(),
        },
        ResultCache::new(
            base.open()?,
            base.hub.clone(),
            base.machine.clone(),
            base.clock.clone(),
        ),
        Box::new(base.trees_source()),
        Box::new(HubHistory {
            base: base.client.base.clone(),
            token: base.token.clone(),
        }),
    ));
    let landing = Arc::new(Landing {
        trees: Arc::new(MachineTrees {
            trees: trees.clone(),
        }),
        gate: Arc::new(ChecksGate {
            checks: checks.clone(),
        }),
        records: Records::new(base.home("landings")),
        pause: Arc::new(|seconds| std::thread::sleep(std::time::Duration::from_secs_f64(seconds))),
    });
    let idle = Arc::new(Idle::new(
        IdleSettings {
            manifest: base.manifest.clone(),
            os: base.os,
            families: base.families(),
            state: base.home("idle.json"),
            outputs: base.home("outputs"),
            hostname: base.options.hostname.clone(),
            cores: per_command,
            ignite: base.options.version.clone(),
        },
        base.clock.clone(),
        Box::new(LiveHost {
            trees: Arc::new(trees.clone()),
            repo: base.options.repo.clone(),
        }),
        checks.clone(),
    )?);
    checks.set_hit_sink(idle.clone());
    Ok((checks, landing, idle))
}

/// The claims, the dispatcher, and the late links both need to the server.
pub struct Desks {
    pub claims: Arc<Claims>,
    pub dispatcher: Arc<Dispatcher>,
    pub tree_desk: Arc<desk::TreeDeskOver>,
    pub host: Arc<desk::DaemonHost>,
}

pub fn desks(base: &Base, ctx: &Context, machinery: &Machinery) -> Result<Desks> {
    let staging = &base.config.repository.staging;
    let tree_desk = Arc::new(desk::TreeDeskOver::new(machinery.trees.clone(), staging));
    let ticket_desk = Arc::new(desk::HubDesk {
        hub: base.client.clone(),
        machine: base.machine.clone(),
    });
    let quiet = Arc::new(desk::QuietDispatcher::default());
    let claims = Claims::new(
        base.clock.clone(),
        ticket_desk,
        tree_desk.clone(),
        quiet,
        base.open()?,
    )?;
    let host = Arc::new(desk::DaemonHost {
        trees: machinery.trees.clone(),
        scheduler: machinery.scheduler.clone(),
        claims: claims.clone(),
        ctx: ctx.clone(),
        server: OnceLock::new(),
    });
    let setup = Setup {
        os: base.os,
        families: base.families().unwrap_or_default(),
    };
    let dispatcher = Arc::new(Dispatcher::new(
        Pool::new(base.open()?),
        host.clone(),
        setup,
    ));
    Ok(Desks {
        claims,
        dispatcher,
        tree_desk,
        host,
    })
}

/// The server with every verb registered, the claims' hook added and the late links set.
pub fn server(
    base: &Base,
    ctx: &Context,
    parts: &loops::Parts,
    desks: &Desks,
) -> Result<Arc<Server>> {
    let mut registry = Registry::default();
    let claims = &parts.claims;
    registry.register(Box::new(StartVerb(claims.clone())));
    registry.register(Box::new(BriefVerb(claims.clone())));
    registry.register(Box::new(ReleaseVerb(claims.clone())));
    registry.register(Box::new(CloseVerb(claims.clone())));
    let checks = &parts.checks;
    // A restarted daemon runs these again under the handle of a run it was cut off in (see
    // `server/resume.rs`): finished checks come from the cache and running commands from
    // their supervisors, and a landing finds where it got to in its record on disk.
    registry.register_resumable(Box::new(TestVerb {
        checks: checks.clone(),
    }));
    registry.register_resumable(Box::new(CheckVerb {
        checks: checks.clone(),
    }));
    registry.register(Box::new(WhyVerb {
        checks: checks.clone(),
    }));
    registry.register_resumable(Box::new(LandVerb {
        landing: parts.landing.clone(),
    }));
    registry.register(Box::new(SyncVerb {
        landing: parts.landing.clone(),
    }));
    registry.register(Box::new(RunVerb));
    registry.register(Box::new(RebuildVerb {
        idle: parts.idle.clone(),
    }));
    registry.register(Box::new(AuditVerb {
        idle: parts.idle.clone(),
    }));
    registry.register(Box::new(DispatchVerb::new(parts.dispatcher.clone())));
    registry.register(Box::new(PoolVerb::new(parts.dispatcher.clone())));
    registry.register(Box::new(TellVerb));
    registry.register(Box::new(crate::justiciar::JusticiarVerb {
        home: parts.home.root.clone(),
        host: parts.reporter.host.clone(),
        dispatcher: parts.dispatcher.clone(),
    }));
    registry.register(Box::new(InstallVerb));
    registry.register(Box::new(DoctorVerb {
        loops: Some(parts.health.clone()),
    }));
    registry.register(Box::new(StatusVerb::new(status_sections(base, parts))));

    let local: Arc<Mutex<Connection>> = Arc::new(Mutex::new(base.open()?));
    let mut server = Server::new(ctx.clone(), registry, Some(local))?;
    server.add_hook(Box::new(ClaimHook(claims.clone())));
    let server = Arc::new(server);
    claims.watch_commands(server.clone());
    let _ = desks.host.server.set(Arc::downgrade(&server));
    let _ = desks.tree_desk.handles.set(server.handles().clone());
    Ok(server)
}

fn status_sections(base: &Base, parts: &loops::Parts) -> Vec<Box<dyn StatusSection>> {
    let status = {
        let scheduler = parts.scheduler.clone();
        Arc::new(move || scheduler.status())
    };
    let configured =
        doctor::configured_tree_names(base.declared.ticket_trees, base.declared.landing_trees);
    vec![
        Box::new(CoresSection(status.clone())),
        Box::new(AdmissionSection(status)),
        Box::new(TreesSection {
            source: Arc::new(GitTrees {
                repo: base.options.repo.clone(),
                dir: base.trees_dir.clone(),
            }),
            configured,
            host: Arc::new(NativeHost),
        }),
        Box::new(LoopsSection(parts.health.clone())),
    ]
}

/// The machine's ID and write token. A machine with no token yet, such as one whose
/// `ignite install` could not reach the hub, registers now.
fn identity(
    options: &DaemonOptions,
    config: &Config,
    os: Os,
    families: Option<&[String]>,
) -> Result<(MachineId, String)> {
    let file = options.home.identity();
    let mut identity = file.load_or_create()?;
    if let Some(token) = &identity.token {
        return Ok((identity.machine, token.clone()));
    }
    let registrar = HubClient::new(&config.hub, "", identity.machine.as_str());
    let registered = registrar
        .register(&MachineRegister {
            machine: identity.machine.clone(),
            name: options.hostname.clone(),
            os,
            cores: doctor::Host::cores(&NativeHost),
            memory_mb: doctor::Host::memory_mb(&NativeHost),
            families: families.map(<[String]>::to_vec).unwrap_or_default(),
            version: options.version.clone(),
        })
        .map_err(explain_refused_registration)?;
    identity.token = Some(registered.token.clone());
    file.save(&identity)?;
    Ok((identity.machine, registered.token))
}

/// A registration the hub refused with 401 or 403: this machine has no token, and the hub holds
/// one for its ID. The hub's own message says to run `ignite install`, which cannot help here.
fn explain_refused_registration(error: Error) -> Error {
    match error {
        Error::Unreachable(message) if message.contains("refused this machine's token") => {
            Error::Invalid(format!(
                "the hub refused to register this machine without its token. {}",
                crate::update::RECOVER
            ))
        }
        other => other,
    }
}

/// The Rust toolchain every input hash includes: `rustc --version`, run in the checkout so that
/// its toolchain file applies, or empty where `rustc` cannot run.
fn toolchain(repo: &Path) -> String {
    Command::new("rustc")
        .arg("--version")
        .current_dir(repo)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refused_registration_says_the_hub_holds_a_token_and_how_to_recover() {
        let refused = Error::Unreachable(
            "the hub refused this machine's token (401); run `ignite install` to register it \
             again: /machines/register"
                .to_owned(),
        );
        let Error::Invalid(text) = explain_refused_registration(refused) else {
            panic!("a clear error is expected");
        };
        assert!(text.contains("already holds a token"), "{text}");
        assert!(text.contains("machine.json"), "{text}");
        let down = Error::Unreachable("connection refused".to_owned());
        assert!(matches!(
            explain_refused_registration(down),
            Error::Unreachable(_)
        ));
    }
}
