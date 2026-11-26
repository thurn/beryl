//! A round of tip verification in its verb process across a restart of the daemon, against the
//! fakes of the long verbs' tests: the tip loop starts the round, the daemon restarts while the
//! round's batch runs, and the restarted loop waits for that round instead of starting one.

use ignite_core::config::Os;
use ignite_core::ids::{CheckName, InputHash, RecordId, Seq};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{NewResult, ResultCost, ResultKey};
use ignite_core::tipround::{
    DeferredPlatform, PlatformVerification, TipCandidate, TipCandidateUpdate, TipCoverageUpdate,
};
use ignite_core::traits::{Git as _, Hub as _};

use super::*;
use crate::tip::{
    ListingPhase, ROUND_VERB, TipCheck, TipLanding, TipLoop, TipOutcome, TipRoundVerb, TipRun,
    TipSet, TipWork,
};

/// One check, `unit:a`, whose run is one command through the round's launcher, which runs until
/// the test finishes it. Counts the runs.
#[derive(Default)]
struct OneCheck {
    runs: Arc<AtomicUsize>,
    remote: Option<(Arc<Switch>, Handle)>,
    block_listing: bool,
    platform: Option<Os>,
    fail_at: Option<Commit>,
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
        machine_relative: false,
        kind: crate::tip::TipCheckKind::Other,
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
                products: None,
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
    fn checks(&self, ctx: &Context, at: &Commit, phase: ListingPhase) -> Result<TipSet> {
        if !phase.includes(false) {
            return Ok(TipSet::default());
        }
        if self.block_listing && self.command(ctx, at, &["cargo", "nextest", "list"]) != Some(0) {
            return Err(Error::Conflict("listing did not finish".to_owned()));
        }
        Ok(TipSet {
            checks: vec![the_check(self.platform)],
            ..TipSet::default()
        })
    }

    fn run(
        &self,
        ctx: &Context,
        at: &Commit,
        checks: &[CheckName],
        _rerun: bool,
        _pre_move: bool,
    ) -> Vec<TipRun> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        let outcome = match self.command(ctx, at, &["cargo", "test"]) {
            Some(0) if self.fail_at.as_ref() == Some(at) => Outcome::Fail,
            Some(0) => Outcome::Pass,
            _ => Outcome::Incomplete,
        };
        checks
            .iter()
            .map(|check| TipRun {
                check: check.clone(),
                input_hash: None,
                failure_key: (outcome == Outcome::Fail).then(|| the_check(self.platform).key),
                outcome,
                output: None,
                reason: None,
                build: None,
            })
            .collect()
    }

    fn landings(&self, _ctx: &Context, _from: &Commit, to: &Commit) -> Result<Vec<TipLanding>> {
        if self.fail_at.as_ref() != Some(to) {
            return Ok(Vec::new());
        }
        Ok([Commit::new("ababa00").unwrap(), to.clone()]
            .into_iter()
            .map(|commit| TipLanding {
                ticket: None,
                commits: vec![commit],
                paths: the_check(self.platform).inputs,
            })
            .collect())
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
                fail_at: self.work.fail_at.clone(),
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
    /// The daemons' clock, which only the test moves.
    clock: Arc<FakeClock>,
    /// What ends a verb process that does not stop: the hung one, if the test made one.
    hung: Arc<Hung>,
}

/// A verb process that took its lock and then hung, until it is ended; counts the verb
/// processes the daemon asked to end.
#[derive(Default)]
struct Hung {
    lock: Mutex<Option<crate::verbproc::VerbLock>>,
    ends: AtomicUsize,
}

impl EndVerbProcess for Hung {
    fn end(&self, _handle: &Handle) -> std::result::Result<Option<String>, String> {
        self.ends.fetch_add(1, Ordering::SeqCst);
        Ok(lock(&self.lock)
            .take()
            .map(|_| "the hung verb process".to_owned()))
    }
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
        let clock = Arc::new(FakeClock::new(T0));
        let mut ctx = context();
        ctx.clock = clock.clone();
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
            clock,
            hung: Arc::new(Hung::default()),
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
            ender: Some(self.hung.clone()),
            stop_wait: crate::stop::STOP_WAIT,
            stall_limit: crate::server::process::STALL_LIMIT,
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
    // A landing updates staging without verifying it. The restarted loop still waits for
    // the command already running at the previous tip.
    machine
        .git
        .set_tip("staging", Commit::new("cccc000").unwrap());
    let (second, second_loop) = machine.daemon();
    machine.switch.set(Some(second.clone()));
    let waiting = turn_on_a_thread(second_loop, ctx.clone());
    std::thread::sleep(Duration::from_millis(300));
    // The round's command runs, so the round is not stuck however long it takes.
    machine
        .clock
        .advance(crate::server::process::STALL_LIMIT.as_secs_f64() + 1.0);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(machine.hung.ends.load(Ordering::SeqCst), 0);
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
    assert_eq!(second_loop.turn(&ctx, 0).unwrap(), None);
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
                checks_deferred: false,
                expected: None,
                replacement: coverage,
            })
            .unwrap()
    );

    // The loop adopts the blocked round from its saved request. The supervisor fake then
    // finishes the listing after the stop has reached the verb.
    let stopping = turn_on_a_thread(tips, ctx.clone());
    assert!(until(|| machine.switch.stops.load(Ordering::SeqCst) > 0));
    machine.commands.finish_all();
    let (tips, turn) = stopping.join().unwrap();
    assert_eq!(
        turn.unwrap(),
        Some((target, TipOutcome::Superseded { by: master }))
    );
    assert!(lock(&machine.landing.owner).is_none());
    assert_eq!(machine.threads.work.runs.load(Ordering::SeqCst), 0);

    let checking = turn_on_a_thread(tips, ctx);
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

#[test]
fn an_adopted_round_blocked_in_listing_selects_a_successor_after_failure() {
    let machine = TipMachine::with_work(
        "replaced-tip-round",
        OneCheck {
            block_listing: true,
            ..OneCheck::default()
        },
    );
    let ctx = machine.threads.ctx.clone();
    let target = machine.git.tip("staging").unwrap();
    let master = machine.git.tip("master").unwrap();
    let candidate = |commit: &Commit| TipCandidate {
        commit: commit.clone(),
        retry_on_new_tip: false,
    };
    assert!(
        machine
            .hub
            .update_tip_candidate(&TipCandidateUpdate {
                expected: None,
                replacement: candidate(&target),
            })
            .unwrap()
    );
    let (first, tips) = machine.daemon();
    machine.switch.set(Some(first.clone()));
    first.start_internal(
        tips.verb.clone(),
        TipRoundVerb::request(&target, Seq(0), &ctx.repo),
    );
    assert!(until(|| machine.commands.begun() == 1));

    machine.switch.set(None);
    drop(first);
    let newer = Commit::new("cccc000").unwrap();
    machine.git.set_tip("staging", newer.clone());
    assert!(
        machine
            .hub
            .update_tip_candidate(&TipCandidateUpdate {
                expected: Some(candidate(&target)),
                replacement: TipCandidate {
                    retry_on_new_tip: true,
                    ..candidate(&target)
                },
            })
            .unwrap()
    );

    let (second, tips) = machine.daemon();
    machine.switch.set(Some(second));
    let stopping = turn_on_a_thread(tips, ctx.clone());
    assert!(until(|| machine.switch.stops.load(Ordering::SeqCst) > 0));
    assert_eq!(
        machine.hub.tip_candidate().unwrap(),
        Some(candidate(&newer))
    );
    machine.commands.finish_all();
    let (tips, turn) = stopping.join().unwrap();
    assert_eq!(
        turn.unwrap(),
        Some((target, TipOutcome::Superseded { by: newer.clone() }))
    );
    assert!(lock(&machine.landing.owner).is_none());
    assert_eq!(machine.git.tip("master").unwrap(), master);
    assert_eq!(machine.threads.work.runs.load(Ordering::SeqCst), 0);

    let checking = turn_on_a_thread(tips, ctx);
    assert!(until(|| machine.commands.begun() == 2));
    machine.commands.finish_all();
    assert!(until(|| machine.commands.begun() == 3));
    machine.commands.finish_all();
    let (_, turn) = checking.join().unwrap();
    assert_eq!(turn.unwrap(), Some((newer, TipOutcome::Verified)));
}

#[test]
fn a_pass_stops_a_running_culprit_search_and_retries_the_same_tip() {
    let target = Commit::new("bbbb000").unwrap();
    let machine = TipMachine::with_work(
        "recovered-tip-round",
        OneCheck {
            fail_at: Some(target.clone()),
            ..OneCheck::default()
        },
    );
    let ctx = machine.threads.ctx.clone();
    let key = the_check(None).key;
    let store = |outcome| {
        ctx.hub
            .store(&[NewResult {
                key: key.clone(),
                outcome,
                machine: ctx.machine.clone(),
                record: RecordId(1),
                output: None,
                cost: ResultCost::default(),
                areas: Vec::new(),
            }])
            .unwrap();
    };
    store(Outcome::Fail);
    let (server, tips) = machine.daemon();
    machine.switch.set(Some(server.clone()));
    server.start_internal(
        tips.verb.clone(),
        TipRoundVerb::request(&target, Seq(0), &ctx.repo),
    );
    assert!(until(|| machine.commands.begun() == 1));
    machine.commands.finish_all();
    assert!(until(|| machine.commands.begun() == 2));

    // The second command is the historical probe. Leave it blocked until the adopted round
    // receives a stop, proving the pass is noticed before the search can finish.
    store(Outcome::Pass);
    let stopping = turn_on_a_thread(tips, ctx.clone());
    assert!(until(|| machine.switch.stops.load(Ordering::SeqCst) > 0));
    machine.commands.finish_all();
    let (mut tips, turn) = stopping.join().unwrap();
    assert_eq!(turn.unwrap(), Some((target.clone(), TipOutcome::Recovered)));
    assert_eq!(
        machine.git.tip("master").unwrap(),
        Commit::new("aaaa000").unwrap()
    );

    assert_eq!(
        tips.turn(&ctx, 0).unwrap(),
        Some((target.clone(), TipOutcome::Verified))
    );
    assert_eq!(machine.git.tip("master").unwrap(), target);
    assert_eq!(machine.commands.begun(), 2);
    assert!(lock(&machine.landing.owner).is_none());
}

/// On 2026-11-25 every culprit search was stopped for a newer tip that failed in turn, so for
/// 16 hours no search finished and nothing was reverted or promoted. A round that has recorded
/// its failed verdict now finishes its search when a newer candidate appears.
#[test]
fn a_newer_tip_does_not_stop_a_running_culprit_search() {
    let target = Commit::new("bbbb000").unwrap();
    let machine = TipMachine::with_work(
        "searching-tip-round",
        OneCheck {
            fail_at: Some(target.clone()),
            ..OneCheck::default()
        },
    );
    let ctx = machine.threads.ctx.clone();
    assert!(
        machine
            .hub
            .update_tip_candidate(&TipCandidateUpdate {
                expected: None,
                replacement: TipCandidate {
                    commit: target.clone(),
                    retry_on_new_tip: false,
                },
            })
            .unwrap()
    );
    let (server, tips) = machine.daemon();
    machine.switch.set(Some(server.clone()));
    server.start_internal(
        tips.verb.clone(),
        TipRoundVerb::request(&target, Seq(0), &ctx.repo),
    );
    assert!(until(|| machine.commands.begun() == 1));
    machine.commands.finish_all();
    assert!(until(|| machine.commands.begun() == 2));

    // The second command is the culprit search's probe. A newer tip lands while it runs, and
    // the loop selects it as the next candidate, but leaves the search running.
    let newer = Commit::new("cccc000").unwrap();
    machine.git.set_tip("staging", newer.clone());
    let searching = turn_on_a_thread(tips, ctx.clone());
    assert!(until(|| machine
        .hub
        .tip_candidate()
        .unwrap()
        .is_some_and(|candidate| candidate.commit == newer)));
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(machine.switch.stops.load(Ordering::SeqCst), 0);
    machine.commands.finish_all();
    let (_, turn) = searching.join().unwrap();
    let (tip, outcome) = turn.unwrap().unwrap();
    assert_eq!(tip, target);
    assert!(
        matches!(outcome, TipOutcome::Failed { .. }),
        "the search finished with {outcome:?}"
    );
}

/// On 2026-11-26 a restarted daemon adopted a tip round whose verb process, started by the
/// build before, had hung: it reported no step and wrote nothing for two hours, and the tip loop
/// waited on it all that time. An adopted round that makes no progress for the stall limit is
/// now ended, its handle ends `incomplete`, and the loop starts a new round.
#[test]
fn an_adopted_tip_round_that_makes_no_progress_is_ended_and_a_new_round_starts() {
    let machine = TipMachine::new("stalled-tip-round");
    let ctx = machine.threads.ctx.clone();
    let tip = machine.git.tip("staging").unwrap();

    // The previous daemon, under build `87631b5`, started a round whose verb process holds its
    // lock and does nothing.
    let (first, _) = machine.daemon();
    let request = TipRoundVerb::request(&tip, Seq(0), &ctx.repo);
    let hung = first.handles.create_for(ROUND_VERB, None, None, Some(&request));
    let process = handle_store::Process {
        pid: 999_999,
        executable: "87631b5".to_owned(),
        protocol: 1,
    };
    handle_store::set_process(&lock(&machine.db), hung.as_str(), &process).unwrap();
    *lock(&machine.hung.lock) = crate::verbproc::hold(&machine.scratch.home, &hung).unwrap();
    drop(first);

    let (second, tips) = machine.daemon();
    machine.switch.set(Some(second));
    let waiting = turn_on_a_thread(tips, ctx.clone());
    std::thread::sleep(Duration::from_millis(200));
    assert!(lock(&machine.hung.lock).is_some(), "ended before the limit");
    assert_eq!(machine.threads.started.load(Ordering::SeqCst), 0);

    machine.clock.advance(crate::server::process::STALL_LIMIT.as_secs_f64() + 1.0);
    let (mut tips, turn) = waiting.join().unwrap();
    let error = turn.unwrap_err().to_string();
    assert!(error.contains("cannot be run again safely"), "{error}");
    assert!(lock(&machine.hung.lock).is_none(), "the hung process was ended");
    let row = handle_store::get(&lock(&machine.db), hung.as_str())
        .unwrap()
        .unwrap();
    assert_eq!(row.state, "incomplete");

    // The loop's next turn starts a new round, which verifies the tip.
    let checking = std::thread::spawn(move || tips.turn(&ctx, 0));
    assert!(
        until(|| machine.commands.begun() == 1),
        "a new round started"
    );
    machine.commands.finish_all();
    assert_eq!(
        checking.join().unwrap().unwrap(),
        Some((tip, TipOutcome::Verified))
    );
    assert_eq!(machine.threads.started.load(Ordering::SeqCst), 1);
}
