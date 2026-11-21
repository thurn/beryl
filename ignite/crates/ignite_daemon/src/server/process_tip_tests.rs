//! A round of tip verification in its verb process across a restart of the daemon, against the
//! fakes of the long verbs' tests: the tip loop starts the round, the daemon restarts while the
//! round's batch runs, and the restarted loop waits for that round instead of starting one.

use ignite_core::ids::{CheckName, InputHash};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::ResultKey;
use ignite_core::traits::Git as _;

use super::*;
use crate::tip::{
    ROUND_VERB, TipCheck, TipLanding, TipLoop, TipOutcome, TipRoundVerb, TipRun, TipSet, TipWork,
};

/// One check, `unit:a`, whose run is one command through the round's launcher, which runs until
/// the test finishes it. Counts the runs.
#[derive(Default)]
struct OneCheck {
    runs: AtomicUsize,
}

fn the_check() -> TipCheck {
    let check = CheckName::new("unit:a").unwrap();
    TipCheck {
        key: ResultKey {
            check: check.clone(),
            hash: InputHash::new("a".repeat(64)).unwrap(),
            platform: None,
        },
        check,
        family: "unit".to_owned(),
        inputs: vec!["src/a".to_owned()],
        runnable: true,
        reuse: true,
    }
}

impl TipWork for OneCheck {
    fn checks(&self, _ctx: &Context, _commit: &Commit) -> Result<TipSet> {
        Ok(TipSet {
            checks: vec![the_check()],
            ..TipSet::default()
        })
    }

    fn run(&self, ctx: &Context, _at: &Commit, checks: &[CheckName], _rerun: bool) -> Vec<TipRun> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        let mut command = spec(&["cargo", "test"]);
        command.class = Class::Tip;
        let result = ctx.launcher.run(&command);
        let outcome = if result.exit == Some(0) {
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
            self.work.clone(),
            self.home.join("daemon.db"),
        );
        let thread = std::thread::spawn(move || {
            process.run(|remote, name, _req| {
                assert_eq!(name, ROUND_VERB);
                let mut ctx = ctx;
                ctx.launcher = Arc::new(RemoteLauncher(remote.clone()));
                Ok(Assembled {
                    verb: Arc::new(TipRoundVerb::new(work, "test", "jarvis", Some(memory))),
                    ctx,
                })
            })
        });
        Ok(Box::new(ThreadChild(Some(thread))))
    }
}

/// What lasts across a restart: the home with its database, the commands, the hub and git.
struct TipMachine {
    scratch: Scratch,
    db: Arc<Mutex<Connection>>,
    commands: Arc<Commands>,
    switch: Arc<Switch>,
    threads: Arc<RoundThreads>,
    hub: Arc<FakeHub>,
    git: Arc<FakeGit>,
    /// The round as the daemon would run it itself, which it never should here.
    in_daemon: Arc<OneCheck>,
}

impl TipMachine {
    fn new(name: &str) -> TipMachine {
        let scratch = Scratch::new(name);
        let db = Arc::new(Mutex::new(
            ignite_store::local::open(&scratch.home.join("daemon.db")).unwrap(),
        ));
        let (hub, git) = (Arc::new(FakeHub::default()), Arc::new(FakeGit::default()));
        git.set_tip("master", Commit::new("aaaa000").unwrap());
        git.set_tip("staging", Commit::new("bbbb000").unwrap());
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
                work: Arc::new(OneCheck::default()),
                started: AtomicUsize::new(0),
            }),
            scratch,
            db,
            switch,
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
                dispatcher: None,
                starter: self.commands.clone(),
                builds: Arc::new(FakeBuilds::default()),
                landing: Arc::new(NoTrees),
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

fn turn_on_a_thread(mut tips: TipLoop, ctx: Context) -> JoinHandle<(TipLoop, Turn)> {
    std::thread::spawn(move || {
        let turn = tips.turn(&ctx, 0);
        (tips, turn)
    })
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
    let cut = turn_on_a_thread(first_loop, ctx.clone());
    assert!(
        until(|| machine.commands.begun() == 1),
        "the batch's command started"
    );

    // The daemon restarts while the batch runs. The first loop's thread stays behind, as a
    // thread of a process that is gone would; nothing reaches it.
    machine.switch.set(None);
    drop(first);
    let (second, second_loop) = machine.daemon();
    machine.switch.set(Some(second.clone()));
    let waiting = turn_on_a_thread(second_loop, ctx.clone());
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
    assert_eq!(second_loop.turn(&ctx, 0).unwrap(), None);
    assert_eq!(machine.threads.started.load(Ordering::SeqCst), 1);
    let (_, cut_turn) = cut.join().unwrap();
    assert_eq!(cut_turn.unwrap(), Some((tip, TipOutcome::Verified)));
}
