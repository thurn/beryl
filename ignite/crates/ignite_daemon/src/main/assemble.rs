//! The steps of [`super::Daemon::start`]: what the machine is ([`Base`]), the machinery that
//! runs commands and keeps trees, the check runner with landing and idle work on top of it, the
//! claims and the dispatcher, and the verbs.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};

use rusqlite::Connection;

use ignite_core::capacity::{Derived, MachineFacts, derive};
use ignite_core::config::{Config, Machine, Os};
use ignite_core::ids::MachineId;
use ignite_core::manifest::Manifest;
use ignite_core::protocol::Class;
use ignite_core::protocol::hub::MachineRegister;
use ignite_core::time::SystemClock;
use ignite_core::traits::{Builds, Clock, Error, Git, Hub, Launcher, Result};
use ignite_git::GitCli;

use super::{DaemonOptions, TreeSetup, desk, loops};
use crate::adopt::AdoptingLauncher;
use crate::build::{BuildManager, BuildSettings, DirectoryTrees, NativeRoom};
use crate::cache::ResultCache;
use crate::checks::sources::HubHistory;
use crate::checks::verbs::{CheckVerb, TestVerb, WhyVerb};
use crate::checks::{CheckSettings, Checks};
use crate::claim::{BriefVerb, ClaimHook, Claims, CloseVerb, ReleaseVerb, StartVerb};
use crate::dispatchd::{Dispatcher, Setup, TellVerb, TurnEndedVerb};
use crate::doctor::{
    self, AdmissionSection, CoresSection, DoctorVerb, GitTrees, LoopsSection, NativeHost,
    StatusSection, StatusVerb, TreesSection,
};
use crate::host::{Host, HostParts, SupervisedStarter, TreesLanding};
use crate::hubclient::HubClient;
use crate::idle::{AuditVerb, Idle, IdleHost, IdleJobVerb, IdleSettings, LiveHost, RebuildVerb};
use crate::land::env::{ChecksGate, LandTrees, MachineTrees};
use crate::land::express::{CoreLender, MachineExpress};
use crate::land::format::MachineFormat;
use crate::land::records::Records;
use crate::land::{LandVerb, Landing, SyncVerb};
use crate::pool::{Pool, PoolVerb};
use crate::sched::{Limits, Scheduler};
use crate::server::{Context, NativeStarter, Processes, Registry, Server, Verb};
use crate::supervise::{RunVerb, SupervisedLauncher};
use crate::tip::{CheckTipWork, TipRoundVerb};
use crate::trees::{TreeConfig, Trees};
use crate::update::InstallVerb;

/// What every part is built from: the configuration, the machine and its hub.
pub struct Base {
    pub options: DaemonOptions,
    pub config: Config,
    pub manifest: Manifest,
    /// The manifests the daemon's build manager, trees and idle work read, which the tip loop
    /// replaces when those in the main checkout change (see `crate::manifests`).
    pub live: crate::manifests::LiveManifest,
    pub declared: Machine,
    pub derived: Derived,
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
    /// Reads the configuration, measures the machine and registers it with the hub if needed.
    pub fn read(options: DaemonOptions) -> Result<Base> {
        let repo = options.repo.clone();
        let config = super::read_config(&repo)?;
        let manifest = super::read_manifest(&repo, &config).map_err(Error::Invalid)?;
        let declared = config
            .machine(&options.hostname)
            .cloned()
            .unwrap_or_default();
        let os = declared
            .os
            .or_else(Os::current)
            .ok_or_else(|| Error::Invalid("this operating system is not supported".into()))?;
        let facts = doctor::machine_facts(&NativeHost, os);
        options.home.create()?;
        let (machine, token) = identity(&options, &config, &facts, declared.families.as_deref())?;
        // The daemon and every verb process read the token again after a 401, since an
        // `ignite install` meanwhile may have saved another one.
        let client = Arc::new(
            HubClient::new(&config.hub, &token, machine.as_str())
                .reading_token_from(options.home.identity()),
        );
        let hub: Arc<dyn Hub> = client.clone();
        let git: Arc<dyn Git> = Arc::new(GitCli {
            repo: repo.clone(),
            remote: config.repository.remote.clone(),
        });
        let derived = derive(&facts);
        let limits = Limits::for_machine(&config, &declared, &manifest, &facts, &derived);
        Ok(Base {
            trees_dir: repo.join(&config.repository.trees),
            options,
            config,
            live: manifest.clone().into(),
            manifest,
            declared,
            derived,
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
    /// The launcher under `launcher`, which starts the commands of verb processes (see
    /// `crate::host`) without the adoption a verb run again needs.
    pub supervised: Arc<SupervisedLauncher>,
    pub builds: Arc<BuildManager>,
    pub trees: Trees,
}

/// The scheduler, the supervisor, builds and the trees. Trees that are missing or were left in
/// the middle of something are created or reset at the tip, before this returns or on a thread
/// of their own, as `setup` says.
pub fn machinery(base: &Base, setup: TreeSetup) -> Result<Machinery> {
    let scheduler = Arc::new(Scheduler::new(
        base.limits.clone(),
        base.clock.clone(),
        // One meter, read by the one sampler thread: a second reading microseconds after the
        // first would see no CPU ticks.
        Arc::new(ignite_os::hostcpu::HostMeter::new()),
    ));
    // The hub and the verb processes a previous daemon started run outside the daemon's tree;
    // their CPU is Ignite's, not load from outside it.
    scheduler.count_as_ignite(crate::sched::roots::native(
        base.options.home.root.clone(),
        base.open()?,
    ));
    Scheduler::sample_every(&scheduler);
    let supervised = Arc::new(
        SupervisedLauncher::new(&base.options.ignite, base.home("runs"))
            .with_scheduler(scheduler.clone())
            .with_agent_priority(
                base.open()?,
                base.home(crate::mirror::FILE),
                base.hub.clone(),
            ),
    );
    loops::collect_unfinished(&supervised);
    // Commands that a verb cut off by the restart had running are handed back to the verb when
    // it runs again (see `crate::adopt`), except those in trees `Trees::setup` resets below: trees
    // left being prebuilt or reset, and landing trees whose owner will not run again. A landing
    // tree whose landing runs again is kept with its commands.
    let reset = crate::trees::reset_at_start(&base.open()?)?;
    // Read before the adopting launcher ends the runs in trees that are reset; the build
    // manager skips those runs itself.
    let unfinished = supervised.unfinished();
    let launcher: Arc<dyn Launcher> = Arc::new(AdoptingLauncher::over_supervised(
        supervised.clone(),
        &reset,
    ));
    let builds = Arc::new(
        BuildManager::new(
            BuildSettings {
                manifest: base.live.clone(),
                checks: base.config.repository.checks.clone(),
                records: base.home("records"),
                cores: base.limits.max_per_command(),
                class: Class::Agent,
                ignite: base.options.ignite.clone(),
                shims: base.home("shims"),
            },
            launcher.clone(),
            base.git.clone(),
            Arc::new(base.trees_source()),
        )
        // What each tree holds and which builds failed survive the restart, and builds a previous
        // daemon left running are waited for instead of run again (see `crate::build`).
        .with_store(base.open()?)
        // A build waits while the disk is nearly full instead of failing partway through it,
        // and the disk loop reads the free space at the repository too.
        .with_room(Arc::new(NativeRoom {
            path: base.options.repo.clone(),
        })),
    );
    builds.rejoin(unfinished, &reset);
    let repo = &base.options.repo;
    let tree_config = TreeConfig::new(repo, &base.config, &base.derived, &base.manifest);
    let trees = Trees::new(
        tree_config,
        base.open()?,
        base.clock.clone(),
        builds.clone(),
    );
    let unfinished = trees.mark_unfinished()?;
    // Before the socket is served, so a build asked for after a restart waits for the copy.
    if let Err(error) = trees.resume_seeds() {
        eprintln!("ignite: seeds a restart cut short were not started again: {error}");
    }
    let staging = &base.config.repository.staging;
    match setup {
        TreeSetup::Now => {
            let tip = base.git.tip(staging)?;
            for tree in &unfinished {
                trees.finish(tree, &tip)?;
            }
        }
        TreeSetup::Background => {
            trees.finish_in_background(base.git.clone(), staging.clone(), unfinished);
        }
    }
    Ok(Machinery {
        scheduler,
        launcher,
        supervised,
        builds,
        trees,
    })
}

/// The check runner, with landing and idle work, which both run checks through it.
pub fn checking(
    base: &Base,
    trees: &Trees,
    lender: Arc<dyn CoreLender>,
) -> Result<(Arc<Checks>, Arc<Landing>, Arc<Idle>)> {
    let landing_trees = Arc::new(MachineTrees {
        trees: trees.clone(),
    });
    checking_with(base, landing_trees, trees, lender, |live| Box::new(live))
}

/// [`checking`], with landing's trees and the idle host given: the machine's trees in the
/// daemon, and in a verb process the same trees with landing trees leased from the daemon (see
/// `crate::verbproc`).
pub fn checking_with(
    base: &Base,
    landing_trees: Arc<dyn LandTrees>,
    trees: &Trees,
    lender: Arc<dyn CoreLender>,
    idle_host: impl FnOnce(LiveHost) -> Box<dyn IdleHost>,
) -> Result<(Arc<Checks>, Arc<Landing>, Arc<Idle>)> {
    let per_command = base.limits.max_per_command();
    let checks = Arc::new(Checks::new(
        CheckSettings {
            manifest: base.manifest.clone(),
            os: base.os,
            families: base.families(),
            hostname: base.options.hostname.clone(),
            toolchain: toolchain(&base.options.repo),
            outputs: base.home("outputs"),
            cores: per_command,
            ignite: base.options.version.clone(),
            ignite_exe: base.options.ignite.clone(),
            shims: base.home("shims"),
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
        trees: landing_trees,
        gate: Arc::new(ChecksGate {
            checks: checks.clone(),
        }),
        records: Records::new(base.home("landings")),
        express: MachineExpress::for_config(
            &base.config,
            &base.options.repo,
            base.derived.express_cores,
            Some(Arc::new(Mutex::new(base.open()?))),
            &base.clock,
            Some(lender),
        ),
        format: MachineFormat::for_config(&base.config),
        pause: Arc::new(|seconds| std::thread::sleep(std::time::Duration::from_secs_f64(seconds))),
    });
    let idle = Arc::new(Idle::new(
        IdleSettings {
            manifest: base.live.clone(),
            os: base.os,
            families: base.families(),
            state: base.home("idle.json"),
            outputs: base.home("outputs"),
            hostname: base.options.hostname.clone(),
            cores: per_command,
            ignite: base.options.version.clone(),
        },
        base.clock.clone(),
        idle_host(LiveHost {
            trees: Arc::new(trees.clone()),
            repo: base.options.repo.clone(),
        }),
        checks.clone(),
    )?);
    checks.set_hit_sink(idle.clone());
    Ok((checks, landing, idle))
}

/// What a verb process runs its verb with (see `crate::verbproc`): the context, with `launcher`
/// and `builds` asking the daemon, and the long verbs, built over the machine's trees as the
/// daemon builds them, with landing trees as `landing` makes them from the machine's. The
/// internal verbs the daemon's loops start come last, such as a round of tip verification (see
/// `crate::tip::TipLoop`). Nothing here changes the trees or starts a loop: that stays the
/// daemon's.
pub fn verb_side(
    base: &Base,
    launcher: Arc<dyn Launcher>,
    builds: Arc<dyn Builds>,
    landing: impl FnOnce(MachineTrees) -> Arc<dyn LandTrees>,
    lender: Arc<dyn CoreLender>,
    idle_host: impl FnOnce(LiveHost) -> Box<dyn IdleHost>,
) -> Result<(Context, Vec<LongVerb>)> {
    let repo = &base.options.repo;
    let tree_config = TreeConfig::new(repo, &base.config, &base.derived, &base.manifest);
    let trees = Trees::new(
        tree_config,
        base.open()?,
        base.clock.clone(),
        builds.clone(),
    );
    let landing_trees = landing(MachineTrees {
        trees: trees.clone(),
    });
    let (checks, landing, idle) =
        checking_with(base, landing_trees.clone(), &trees, lender, idle_host)?;
    let ctx = Context {
        config: base.config.clone(),
        repo: repo.clone(),
        machine: base.machine.clone(),
        clock: base.clock.clone(),
        hub: base.hub.clone(),
        git: base.git.clone(),
        launcher,
        builds,
        session: None,
        tree: None,
    };
    let mut verbs = long_verbs(&checks, &landing, &idle);
    let tip_work = CheckTipWork {
        checks: checks.clone(),
        trees: landing_trees,
        landing_trees: trees.landing_count(),
    };
    let round = TipRoundVerb::new(
        Arc::new(tip_work),
        base.options.version.clone(),
        base.options.hostname.clone(),
        Some(base.home("daemon.db")),
    );
    verbs.push((Box::new(round), false));
    Ok((ctx, verbs))
}

/// The claims, the dispatcher, and the late links both need to the server.
pub struct Desks {
    /// The launcher that starts the commands of verb processes, passed on to the server.
    pub supervised: Arc<SupervisedLauncher>,
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
    let claims = Claims::new(
        base.clock.clone(),
        ticket_desk,
        tree_desk.clone(),
        base.open()?,
    )?;
    let host = Arc::new(desk::DaemonHost {
        trees: machinery.trees.clone(),
        scheduler: machinery.scheduler.clone(),
        worker_ceiling: base.derived.worker_ceiling,
        queue_recovery: Mutex::default(),
        claims: claims.clone(),
        ctx: ctx.clone(),
        server: OnceLock::new(),
        results: std::sync::Mutex::new(base.open()?),
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
    claims.gate_starts(dispatcher.clone());
    Ok(Desks {
        supervised: machinery.supervised.clone(),
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
    // A `start` that only waited for a free tree when the daemon restarted is run again by the
    // new daemon, which finds the tree if one was leased to the session meanwhile.
    registry.register_resumable(Box::new(StartVerb(claims.clone())));
    registry.register(Box::new(BriefVerb(claims.clone())));
    registry.register(Box::new(ReleaseVerb(claims.clone())));
    registry.register(Box::new(CloseVerb(claims.clone())));
    for (verb, resumable) in long_verbs(&parts.checks, &parts.landing, &parts.idle) {
        if resumable {
            registry.register_resumable(verb);
        } else {
            registry.register(verb);
        }
    }
    registry.register(Box::new(WhyVerb {
        checks: parts.checks.clone(),
    }));
    registry.register(Box::new(SyncVerb {
        landing: parts.landing.clone(),
    }));
    registry.register(Box::new(RemovedDispatchVerb));
    registry.register(Box::new(
        PoolVerb::new(parts.dispatcher.clone()).ending(parts.emergencies.clone()),
    ));
    registry.register(Box::new(crate::emergency::EmergencyVerb(
        parts.emergencies.clone(),
    )));
    registry.register(Box::new(TellVerb));
    registry.register(Box::new(TurnEndedVerb::new(
        parts.dispatcher.clone(),
        parts.reporter.host.clone(),
    )));
    registry.register(Box::new(crate::justiciar::JusticiarVerb {
        home: parts.home.root.clone(),
        host: parts.reporter.host.clone(),
        dispatcher: parts.dispatcher.clone(),
    }));
    registry.register(Box::new(InstallVerb));
    registry.register(Box::new(DoctorVerb {
        loops: Some(parts.health.clone()),
        mirror: Some(parts.home.root.join(crate::mirror::FILE)),
        emergency: Some(parts.emergencies.clone()),
        managed_trees: Some(parts.trees.clone()),
    }));
    registry.register(Box::new(
        StatusVerb::new(status_sections(base, parts)).running_under(executable_of(base)?),
    ));
    // `ignite wait deploy-<commit>`, which `ignite land` names when it moves the release.
    crate::deploy::install(Arc::new(crate::deploy::DeployWaiter {
        state: parts.update.clone(),
        hub: parts.ctx.hub.clone(),
        clock: base.clock.clone(),
        pause: Arc::new(|seconds| std::thread::sleep(std::time::Duration::from_secs_f64(seconds))),
    }));
    // The calls of verb processes, which run long verbs outside the daemon (see `crate::host`).
    let host = Host::new(HostParts {
        dispatcher: Some(desks.dispatcher.clone()),
        starter: Arc::new(SupervisedStarter::new(
            desks.supervised.clone(),
            Some(parts.scheduler.clone()),
        )),
        builds: ctx.builds.clone(),
        landing: Arc::new(TreesLanding {
            trees: parts.trees.clone(),
            db: Arc::new(Mutex::new(base.open()?)),
        }),
        db: Arc::new(Mutex::new(base.open()?)),
        steps: crate::steps::global(),
        clock: base.clock.clone(),
    });
    host.register(&mut registry);

    let local: Arc<Mutex<Connection>> = Arc::new(Mutex::new(base.open()?));
    // The long verbs run in verb processes started from this executable (see
    // `server/process.rs`).
    let home = base.options.home.root.clone();
    let processes = Processes::new(
        host.clone(),
        Arc::new(NativeStarter {
            executable: base.options.ignite.clone(),
            home: home.clone(),
            repo: base.options.repo.clone(),
            hostname: base.options.hostname.clone(),
        }),
        home,
    )
    .with_version(&base.options.version)
    .with_git(base.git.clone());
    let mut server = Server::with_processes(
        ctx.clone(),
        registry,
        Some(local),
        crate::steps::global(),
        Some(processes),
    )?;
    server.add_hook(Box::new(ClaimHook(claims.clone())));
    server.add_hook(Box::new(parts.titles.clone()));
    server.enforce_emergency(parts.emergencies.clone());
    let server = Arc::new(server);
    host.set_handles(server.handles().clone());
    claims.watch_commands(server.clone());
    let _ = desks.host.server.set(Arc::downgrade(&server));
    let _ = desks.tree_desk.handles.set(server.handles().clone());
    Ok(server)
}

/// A verb that runs long, with whether a restarted daemon runs it again (see [`long_verbs`]).
pub type LongVerb = (Box<dyn Verb>, bool);

/// The verbs that run long, each with whether a restarted daemon runs it again under the handle
/// of a run it was cut off in (see `server/resume.rs`): finished checks come from the cache and
/// running commands from their supervisors, and a landing finds where it got to in its record on
/// disk. The daemon registers them, and a verb process runs one of them (see `crate::verbproc`).
pub fn long_verbs(checks: &Arc<Checks>, landing: &Arc<Landing>, idle: &Arc<Idle>) -> Vec<LongVerb> {
    vec![
        (
            Box::new(TestVerb {
                checks: checks.clone(),
                express: landing.express.clone(),
            }),
            true,
        ),
        (
            Box::new(CheckVerb {
                checks: checks.clone(),
                express: landing.express.clone(),
            }),
            true,
        ),
        (
            Box::new(LandVerb {
                landing: landing.clone(),
            }),
            true,
        ),
        (Box::new(RunVerb), false),
        (Box::new(RebuildVerb { idle: idle.clone() }), false),
        (Box::new(AuditVerb { idle: idle.clone() }), false),
        (Box::new(IdleJobVerb { idle: idle.clone() }), false),
    ]
}

/// The commit of the executable a handle's verb runs under: the one its verb process recorded,
/// or this daemon's own for a verb the daemon runs.
fn executable_of(base: &Base) -> Result<doctor::ExecutableOf> {
    let db = Mutex::new(base.open()?);
    let own = base.options.version.clone();
    Ok(Arc::new(move |handle: &str| {
        let row = ignite_store::local::handles::get(&crate::handles::lock(&db), handle)
            .ok()
            .flatten()?;
        Some(row.executable.unwrap_or_else(|| own.clone()))
    }))
}

fn status_sections(base: &Base, parts: &loops::Parts) -> Vec<Box<dyn StatusSection>> {
    let status = {
        let scheduler = parts.scheduler.clone();
        Arc::new(move || scheduler.status())
    };
    vec![
        Box::new(CoresSection(status.clone())),
        Box::new(AdmissionSection(status)),
        Box::new(TreesSection {
            source: Arc::new(GitTrees {
                repo: base.options.repo.clone(),
                dir: base.trees_dir.clone(),
            }),
            landing_trees: base.derived.landing_trees,
            host: Arc::new(NativeHost),
        }),
        Box::new(crate::emergency::EmergencySection(
            parts.emergencies.clone(),
        )),
        Box::new(crate::pool::PauseSection {
            dispatcher: parts.dispatcher.clone(),
            hub: parts.ctx.hub.clone(),
            clock: parts.ctx.clock.clone(),
        }),
        Box::new(LoopsSection(parts.health.clone())),
        Box::new(doctor::TipRoundSection {
            hub: parts.ctx.hub.clone(),
            green: green_at(base, parts),
        }),
        Box::new(crate::deploy::ReleaseSection {
            state: parts.update.clone(),
            git: parts.ctx.git.clone(),
            staging: base.config.repository.staging.clone(),
            hub: parts.ctx.hub.clone(),
            clock: base.clock.clone(),
        }),
    ]
}

/// Where `repository.green` is and how many commits the `staging` tip is ahead of it, as this
/// checkout last fetched them; `None` when no green branch is configured.
fn green_at(base: &Base, parts: &loops::Parts) -> Option<doctor::GreenAt> {
    let green = base.config.repository.green.clone()?;
    let staging = base.config.repository.staging.clone();
    let git = parts.ctx.git.clone();
    let repo = base.options.repo.clone();
    Some(Arc::new(move || {
        let at = git.known_tip(&green)?;
        let tip = git.known_tip(&staging)?;
        let behind = ignite_git::refs::commits_between(&repo, &at, &tip)?.len();
        Ok((at, behind))
    }))
}

/// The machine's ID and write token. A machine with no token yet, such as one whose
/// `ignite install` could not reach the hub, registers now.
fn identity(
    options: &DaemonOptions,
    config: &Config,
    facts: &MachineFacts,
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
            os: facts.os,
            cores: facts.logical_cores,
            memory_mb: facts.memory_mb,
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

/// Answers older clients until they stop sending desktop dispatch requests.
struct RemovedDispatchVerb;

impl crate::server::Verb for RemovedDispatchVerb {
    fn name(&self) -> &'static str {
        "dispatch"
    }

    fn needs_the_tree_lease(&self) -> bool {
        false
    }

    fn run(
        &self,
        _ctx: &Context,
        _req: &ignite_core::protocol::client::Request,
    ) -> ignite_core::protocol::client::Reply {
        use ignite_core::protocol::client::{Exit, Reply};
        Reply::text(Exit::Usage, "the desktop dispatcher was removed")
    }
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
