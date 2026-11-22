//! A round of tip verification in its verb process across a restart of the daemon, against the
//! fakes of the long verbs' tests: the tip loop starts the round, the daemon restarts while the
//! round's batch runs, and the restarted loop waits for that round instead of starting one.

use ignite_core::config::Os;
use ignite_core::ids::{CheckName, InputHash, Seq};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::ResultKey;
use ignite_core::tipround::{DeferredPlatform, PlatformVerification, TipCoverageUpdate};
use ignite_core::traits::{Git as _, Hub as _};

use super::*;
use crate::tip::{
    ROUND_VERB, TipCheck, TipLanding, TipLoop, TipOutcome, TipRoundVerb, TipRun, TipSet, TipWork,
};

/// One check, `unit:a`, whose run is one command through the round's launcher, which runs until
/// the test finishes it. Counts the runs.
#[derive(Default)]
struct OneCheck {
    runs: Arc<AtomicUsize>,
    remote: Option<(Arc<Switch>, Handle)>,
    block_listing: bool,
    platform: Option<Os>,
}

fn the_check(platform: Option<Os>) -> TipCheck {
    let check = CheckName::new("unit:a").unwrap();
    TipCheck {
        key: ResultKey {
            check: check.clone(),
            hash: InputHash::new("a".repeat(64)).unwrap(),
            platform,
        },
        check,
        family: "unit".to_owned(),
        inputs: vec!["src/a".to_owned()],
        runnable: true,
        reuse: true,
    }
}

impl OneCheck {
    /// Runs a command under a landing-tree lease, as the real listing and checks do.
    fn command(&self, ctx: &Context, at: &Commit, args: &[&str]) -> Option<i32> {
        let mut command = spec(args);
        command.class = Class::Tip;
        let Some((host, handle)) = &self.remote else {
            return ctx.launcher.run(&command).exit;
        };
        let LeaseLandingReply::Leased { tree, path, .. } = host
            .lease_landing(&LeaseLanding {
                handle: handle.clone(),
                holder: format!("tip {at}"),
                commit: at.clone(),
                keep_free: 0,
            })
            .unwrap()
        else {
            panic!("the round's landing tree is still held");
        };
        command.cwd = path;
        command.tree = Some(tree.clone());
        let result = ctx.launcher.run(&command);
        host.release_landing(&ReleaseLanding {
            handle: handle.clone(),
            tree,
            built: Some(at.clone()),
        })
        .unwrap();
        result.exit
    }
}

impl TipWork for OneCheck {
    fn checks(&self, ctx: &Context, at: &Commit) -> Result<TipSet> {
        if self.block_listing && self.command(ctx, at, &["cargo", "nextest", "list"]) != Some(0) {
            return Err(Error::Conflict("listing did not finish".to_owned()));
        }
        Ok(TipSet {
            checks: vec![the_check(self.platform)],
            ..TipSet::default()
        })
    }

    fn run(&self, ctx: &Context, at: &Commit, checks: &[CheckName], _rerun: bool) -> Vec<TipRun> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        let outcome = if self.command(ctx, at, &["cargo", "test"]) == Some(0) {
            Outcome::Pass
        } else {
            Outcome::Incomplete
        };
        checks
            .iter()
            .map(|check| TipRun {
                check: check.clone(),
                outcome,
                output: None,
                reason: None,
                build: None,
            })
            .collect()
    }

    fn landings(&self, _ctx: &Context, _from: &Commit, _to: &Commit) -> Result<Vec<TipLanding>> {
        Ok(Vec::new())
    }
}

/// Starts each round's verb process on a thread of the test, reaching the daemon through the
/// switch, with the machine's hub and git.
struct RoundThreads {
    home: PathBuf,
    db: Arc<Mutex<Connection>>,
    switch: Arc<Switch>,
    ctx: Context,
    work: Arc<OneCheck>,
    started: AtomicUsize,
}

impl VerbStarter for RoundThreads {
    fn start(&self, handle: &Handle) -> io::Result<Box<dyn VerbChild>> {
        self.started.fetch_add(1, Ordering::SeqCst);
        let process = Process {
            handle: handle.clone(),
            home: self.home.clone(),
            db: self.db.clone(),
            host: self.switch.clone(),
            executable: "87631b5".to_owned(),
            clock: Arc::new(FakeClock::new(T0)),
            timing: ProcessTiming {
                retry_for: Duration::from_secs(20),
                pause: Duration::from_millis(5),
                poll: Duration::from_millis(10),
            },
        };
        let (ctx, work, memory) = (
            self.ctx.clone(),
            OneCheck {
                runs: self.work.runs.clone(),
                remote: Some((self.switch.clone(), handle.clone())),
                block_listing: self.work.block_listing,
                platform: self.work.platform,
            },
            self.home.join("daemon.db"),
        );
        let thread = std::thread::spawn(move || {
            process.run(|remote, name, _req| {
                assert_eq!(name, ROUND_VERB);
                let mut ctx = ctx;
                ctx.launcher = Arc::new(RemoteLauncher(remote.clone()));
                Ok(Assembled {
                    verb: Arc::new(TipRoundVerb::new(
                        Arc::new(work),
                        "test",
                        "jarvis",
                        Some(memory),
                    )),
                    ctx,
                })
            })
        });
        Ok(Box::new(ThreadChild(Some(thread))))
    }
}

#[derive(Default)]
struct OneLandingTree {
    owner: Mutex<Option<Handle>>,
}

impl LandingTrees for OneLandingTree {
    fn lease(&self, _: &str, owner: &Handle, at: &Commit, _: usize) -> Result<Option<TreeLease>> {
        let mut held = lock(&self.owner);
        if held.is_some() {
            return Ok(None);
        }
        *held = Some(owner.clone());
        Ok(Some(TreeLease {
            tree: TreeId::new("landing-1").unwrap(),
            path: PathBuf::from("/repo/trees/landing-1"),
            branch: "landing-1".to_owned(),
            at: at.clone(),
        }))
    }

    fn release(&self, owner: &Handle, _: &TreeId, _: Option<&Commit>) -> Result<()> {
        let mut held = lock(&self.owner);
        assert_eq!(held.take().as_ref(), Some(owner));
        Ok(())
    }
}

/// What lasts across a restart: the home with its database, the commands, the hub and git.
struct TipMachine {
    scratch: Scratch,
    db: Arc<Mutex<Connection>>,
    commands: Arc<Commands>,
    landing: Arc<OneLandingTree>,
    switch: Arc<Switch>,
    threads: Arc<RoundThreads>,
    hub: Arc<FakeHub>,
    git: Arc<FakeGit>,
    /// The round as the daemon would run it itself, which it never should here.
    in_daemon: Arc<OneCheck>,
}

impl TipMachine {
    fn new(name: &str) -> TipMachine {
        Self::with_work(name, OneCheck::default())
    }

    fn with_work(name: &str, work: OneCheck) -> TipMachine {
        let scratch = Scratch::new(name);
        let db = Arc::new(Mutex::new(
            ignite_store::local::open(&scratch.home.join("daemon.db")).unwrap(),
        ));
        let (hub, git) = (Arc::new(FakeHub::default()), Arc::new(FakeGit::default()));
        let master = Commit::new("aaaa000").unwrap();
        let staging = Commit::new("bbbb000").unwrap();
        git.set_not_ancestor(&staging, &master);
        git.set_tip("master", master);
        git.set_tip("staging", staging);
        let mut ctx = context();
        ctx.hub = hub.clone();
        ctx.git = git.clone();
        let switch = Arc::new(Switch::default());
        TipMachine {
            commands: Arc::new(Commands {
                runs: scratch.home.join("runs"),
                ..Commands::default()
            }),
            threads: Arc::new(RoundThreads {
                home: scratch.home.clone(),
                db: db.clone(),
                switch: switch.clone(),
                ctx,
                work: Arc::new(work),
                started: AtomicUsize::new(0),
            }),
            scratch,
            db,
            switch,
            landing: Arc::new(OneLandingTree::default()),
            hub,
            git,
            in_daemon: Arc::new(OneCheck::default()),
        }
    }

    /// A daemon starting on the machine, with its tip loop.
    fn daemon(&self) -> (Arc<Server>, TipLoop) {
        let steps = Arc::new(Steps::new(Arc::new(FakeClock::new(T0))));
        let host = Host::with_timing(
            HostParts {
                headless_dispatch: None,
                starter: self.commands.clone(),
                builds: Arc::new(FakeBuilds::default()),
                landing: self.landing.clone(),
                db: self.db.clone(),
                steps: steps.clone(),
                clock: Arc::new(FakeClock::new(T0)),
            },
            HostTiming {
                block: Duration::from_millis(200),
                abandon: Duration::from_secs(5),
                watch: Duration::from_millis(20),
                grace: Duration::from_millis(50),
            },
        );
        let mut registry = Registry::default();
        host.register(&mut registry);
        let processes = Processes {
            host,
            starter: self.threads.clone(),
            home: self.scratch.home.clone(),
            verbs: BTreeSet::from([ROUND_VERB.to_owned()]),
            watch: Duration::from_millis(20),
            version: None,
            git: None,
            ender: Some(Arc::new(EndsNothing)),
            stop_wait: crate::stop::STOP_WAIT,
        };
        let server = Arc::new(
            Server::with_processes(
                self.threads.ctx.clone(),
                registry,
                Some(self.db.clone()),
                steps,
                Some(processes),
            )
            .unwrap(),
        );
        let verb = TipRoundVerb::new(
            self.in_daemon.clone(),
            "test",
            "jarvis",
            Some(self.scratch.home.join("daemon.db")),
        );
        (server.clone(), TipLoop::new(server, Arc::new(verb)))
    }
}

type Turn = Result<Option<(Commit, TipOutcome)>>;

fn turn_on_a_thread(
    mut tips: TipLoop,
    ctx: Context,
    before_start: impl FnOnce() + Send + 'static,
) -> JoinHandle<(TipLoop, Turn)> {
    std::thread::spawn(move || {
        let turn = tips.turn(&ctx, 0, before_start);
        (tips, turn)
    })
}

#[test]
fn a_new_tip_round_makes_room_before_starting_its_verb() {
    let machine = TipMachine::new("tip-round-disk");
    let (server, tips) = machine.daemon();
    machine.switch.set(Some(server));
    let rounds = machine.threads.clone();
    let sweeps = Arc::new(AtomicUsize::new(0));
    let counted = sweeps.clone();
    let checking = turn_on_a_thread(tips, machine.threads.ctx.clone(), move || {
        assert_eq!(rounds.started.load(Ordering::SeqCst), 0);
        counted.fetch_add(1, Ordering::SeqCst);
    });
    assert!(until(|| machine.commands.begun() == 1));
    assert_eq!(sweeps.load(Ordering::SeqCst), 1);
    machine.commands.finish_all();
    let (_, turn) = checking.join().unwrap();
    assert_eq!(turn.unwrap().unwrap().1, TipOutcome::Verified);
}

/// The acceptance test: a round cut by a restart of the daemon finishes in its verb process,
/// runs its batch once under one claim and sends one round record, and the restarted loop waits
/// for it instead of starting a second round.
#[test]
fn a_tip_round_survives_a_restart_and_the_new_loop_waits_for_it() {
    let machine = TipMachine::new("tip-round");
    let ctx = machine.threads.ctx.clone();
    let tip = Commit::new("bbbb000").unwrap();
    let (first, first_loop) = machine.daemon();
    machine.switch.set(Some(first.clone()));
    let cut = turn_on_a_thread(first_loop, ctx.clone(), || {});
    assert!(
        until(|| machine.commands.begun() == 1),
        "the batch's command started"
    );

    // The daemon restarts while the batch runs. The first loop's thread stays behind, as a
    // thread of a process that is gone would; nothing reaches it.
    machine.switch.set(None);
    drop(first);
    // A landing updates staging without verifying it. The restarted loop still waits for
    // the command already running at the previous tip.
    machine
        .git
        .set_tip("staging", Commit::new("cccc000").unwrap());
    let (second, second_loop) = machine.daemon();
    machine.switch.set(Some(second.clone()));
    let waiting = turn_on_a_thread(second_loop, ctx.clone(), || {
        panic!("a round in progress does not ask for a disk sweep")
    });
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(machine.threads.started.load(Ordering::SeqCst), 1);
    assert_eq!(machine.commands.begun(), 1, "no second round started");

    machine.commands.finish_all();
    let (mut second_loop, turn) = waiting.join().unwrap();
    assert_eq!(turn.unwrap(), Some((tip.clone(), TipOutcome::Verified)));
    assert_eq!(machine.git.tip("master").unwrap(), tip);
    assert_eq!(machine.threads.work.runs.load(Ordering::SeqCst), 1);
    assert_eq!(machine.in_daemon.runs.load(Ordering::SeqCst), 0);
    assert_eq!(machine.commands.begun(), 1, "the command ran once");
    assert_eq!(machine.hub.tip_claims().len(), 1, "one batch was claimed");
    assert_eq!(machine.hub.tip_rounds().len(), 1, "one round record");

    // The tip is settled: the next turn starts nothing.
    machine.git.set_tip("staging", tip.clone());
    assert_eq!(
        second_loop
            .turn(&ctx, 0, || panic!("no round is due"))
            .unwrap(),
        None
    );
    assert_eq!(machine.threads.started.load(Ordering::SeqCst), 1);
    let (_, cut_turn) = cut.join().unwrap();
    assert_eq!(cut_turn.unwrap(), Some((tip, TipOutcome::Verified)));
}

#[test]
fn a_round_blocked_in_listing_yields_to_master_and_the_latest_tip_runs() {
    let platform = Os::current().expect("the test machine's operating system");
    let machine = TipMachine::with_work(
        "superseded-tip-round",
        OneCheck {
            block_listing: true,
            platform: Some(platform),
            ..OneCheck::default()
        },
    );
    let ctx = machine.threads.ctx.clone();
    let target = machine.git.tip("staging").unwrap();
    let (server, tips) = machine.daemon();
    machine.switch.set(Some(server.clone()));
    let handle = server.start_internal(
        tips.verb.clone(),
        TipRoundVerb::request(&target, Seq(0), &ctx.repo),
    );
    assert!(until(|| machine.commands.begun() == 1));
    assert_eq!(*lock(&machine.landing.owner), Some(handle.clone()));

    let master = Commit::new("cccc000").unwrap();
    let staging = Commit::new("dddd000").unwrap();
    machine.git.set_tip("master", master.clone());
    machine.git.set_tip("staging", staging.clone());
    machine.git.set_not_ancestor(&staging, &master);
    let last_good = Commit::new("aaaa000").unwrap();
    let coverage = PlatformVerification {
        through: master.clone(),
        deferred: Some(DeferredPlatform {
            since: target.clone(),
            last_good,
        }),
    };
    assert!(
        machine
            .hub
            .update_tip_coverage(&TipCoverageUpdate {
                platform,
                expected: None,
                replacement: coverage,
            })
            .unwrap()
    );

    // The loop adopts the blocked round from its saved request. The supervisor fake then
    // finishes the listing after the stop has reached the verb.
    let stopping = turn_on_a_thread(tips, ctx.clone(), || {
        panic!("the old round is still active")
    });
    assert!(until(|| machine.switch.stops.load(Ordering::SeqCst) > 0));
    machine.commands.finish_all();
    let (tips, turn) = stopping.join().unwrap();
    assert_eq!(
        turn.unwrap(),
        Some((target, TipOutcome::Superseded { by: master }))
    );
    assert!(lock(&machine.landing.owner).is_none());
    assert_eq!(machine.threads.work.runs.load(Ordering::SeqCst), 0);

    let landing = machine.landing.clone();
    let checking = turn_on_a_thread(tips, ctx, move || {
        assert!(
            lock(&landing.owner).is_none(),
            "the disk sweep can use the tree"
        );
    });
    assert!(until(|| machine.commands.begun() == 2));
    machine.commands.finish_all();
    assert!(until(|| machine.commands.begun() == 3));
    machine.commands.finish_all();
    let (_, turn) = checking.join().unwrap();
    assert_eq!(turn.unwrap(), Some((staging.clone(), TipOutcome::Verified)));
    let coverage = machine.hub.tip_coverage().unwrap();
    assert_eq!(
        coverage.platforms[&platform],
        PlatformVerification {
            through: staging,
            deferred: None,
        }
    );
}
