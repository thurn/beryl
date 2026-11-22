//! Idle jobs in verb processes: the loop's driver starts a job without waiting for it, finds a
//! running job across a restart and starts no second one, a failed job fails the round that
//! settles it, and the processes that share the state file lose none of each other's changes.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use ignite_core::fakes::{FakeBuilds, FakeHost};
use ignite_core::ids::{Commit, Handle, TreeId};
use ignite_core::protocol::Usage;
use ignite_core::protocol::client::{Exit, Reply};
use ignite_core::protocol::host::{Hello, LeaseLandingReply, PROTOCOL_VERSION, RequestIds};
use ignite_core::protocol::hub::CompileUnit;
use ignite_core::traits::{BuildResult, Builds, Result};

use super::{Fixture, commit, request};
use crate::idle::launch::{IdleJobVerb, JobRunner, JobWait};
use crate::idle::state::NewCheck;
use crate::idle::{IdleHost, IdleJob, LiveHost, RebuildVerb, RemoteIdleHost};
use crate::server::Verb;
use crate::steps::{Kind, Steps, Work};
use crate::trees::{Trees, command_lock, tests::Fixture as TreeFixture};
use crate::verbproc::{Link, Remote, Timing};

/// The verb processes of every daemon in a test: they outlive each simulated restart, as the
/// real ones do, and a job finishes when the test says so.
#[derive(Default)]
struct Processes {
    started: Mutex<Vec<(Handle, IdleJob)>>,
    reply: Mutex<Option<Reply>>,
    waits: AtomicUsize,
}

impl Processes {
    fn finish(&self, reply: Reply) {
        *self.reply.lock().unwrap() = Some(reply);
    }

    fn starts(&self) -> usize {
        self.started.lock().unwrap().len()
    }
}

/// What one daemon's idle loop starts jobs with.
struct Runner(Arc<Processes>);

impl JobRunner for Runner {
    fn start(&self, job: &IdleJob, named: &mut dyn FnMut(&Handle)) -> Result<Handle> {
        let handle = Handle::new(format!("I-{}", self.0.starts() + 1)).unwrap();
        named(&handle);
        self.0
            .started
            .lock()
            .unwrap()
            .push((handle.clone(), job.clone()));
        Ok(handle)
    }

    fn wait(&self, handle: &Handle, seconds: u32) -> JobWait {
        let known = self
            .0
            .started
            .lock()
            .unwrap()
            .iter()
            .any(|(h, _)| h == handle);
        if !known {
            return JobWait::Gone;
        }
        let since = Instant::now();
        loop {
            if let Some(reply) = self.0.reply.lock().unwrap().clone() {
                return JobWait::Done(reply);
            }
            // Real waits last seconds; a test's last a moment.
            if seconds == 0 || since.elapsed() >= Duration::from_millis(20) {
                return JobWait::Running;
            }
            self.0.waits.fetch_add(1, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(2));
        }
    }
}

/// A round starts a job and returns at once, so a job that runs for half an hour never holds
/// a round open past the loop's stall limit. A restarted daemon finds the recorded handle and,
/// while the job runs, starts nothing.
#[test]
fn a_round_does_not_wait_for_its_job_and_a_restart_starts_no_second_one() {
    let f = Fixture::new();
    let processes = Arc::new(Processes::default());
    let pool = crate::pool::Pool::new(ignite_store::local::open_in_memory().unwrap());

    // The first daemon's round starts the job and returns while it still runs.
    f.idle()
        .drive(&f.ctx, &Runner(processes.clone()), &pool)
        .unwrap();
    assert_eq!(processes.starts(), 1);
    assert!(f.dir.join("state/idle-job.handle").exists());
    assert_eq!(
        processes.waits.load(Ordering::SeqCst),
        0,
        "the round waited"
    );

    // The restarted daemon's rounds find the running job and start no second one.
    let restarted = f.idle();
    assert!(restarted.settle(&Runner(processes.clone())).unwrap());
    restarted
        .drive(&f.ctx, &Runner(processes.clone()), &pool)
        .unwrap();
    assert_eq!(
        processes.starts(),
        1,
        "the restarted loop started a second job"
    );

    // Once the job has finished, a round settles it and forgets its handle.
    processes.finish(Reply::text(Exit::Success, "done"));
    assert!(!restarted.settle(&Runner(processes.clone())).unwrap());
    assert!(
        !f.dir.join("state/idle-job.handle").exists(),
        "the finished job's handle is still recorded"
    );
}

#[test]
fn a_job_that_finished_while_no_daemon_ran_is_not_started_again() {
    let f = Fixture::new();
    let processes = Arc::new(Processes::default());
    let pool = crate::pool::Pool::new(ignite_store::local::open_in_memory().unwrap());
    let handle = Handle::new("I-1").unwrap();
    processes
        .started
        .lock()
        .unwrap()
        .push((handle.clone(), IdleJob::Audit));
    std::fs::create_dir_all(f.dir.join("state")).unwrap();
    std::fs::write(f.dir.join("state/idle-job.handle"), handle.as_str()).unwrap();
    processes.finish(Reply::text(Exit::Success, "done"));

    f.idle()
        .drive(&f.ctx, &Runner(processes.clone()), &pool)
        .unwrap();

    assert_eq!(processes.starts(), 1, "a second job started");
    assert!(!f.dir.join("state/idle-job.handle").exists());
}

#[test]
fn a_job_that_a_stop_ended_fails_the_round_that_settles_it_and_runs_again_later() {
    let f = Fixture::new();
    let processes = Arc::new(Processes::default());
    let pool = crate::pool::Pool::new(ignite_store::local::open_in_memory().unwrap());
    f.idle()
        .drive(&f.ctx, &Runner(processes.clone()), &pool)
        .unwrap();
    // The scheduler ended the job's commands, so the job's reply says it failed.
    processes.finish(Reply::text(Exit::Unreachable, "the command was ended"));

    let result = f.idle().drive(&f.ctx, &Runner(processes.clone()), &pool);

    assert!(result.is_err(), "a failed job succeeded");
    assert!(!f.dir.join("state/idle-job.handle").exists());
    // The next round starts the next job.
    processes.reply.lock().unwrap().take();
    f.idle()
        .drive(&f.ctx, &Runner(processes.clone()), &pool)
        .unwrap();
    assert_eq!(processes.starts(), 2);
}

struct CleanupBuild(Trees, mpsc::SyncSender<std::fs::File>);

impl Builds for CleanupBuild {
    fn ensure(&self, tree: &TreeId, _: &str, _: &[String]) -> BuildResult {
        let command = command_lock(&self.0.path(tree).unwrap()).unwrap();
        command.lock_shared().unwrap();
        self.1.send(command).unwrap();
        let stopped = self
            .0
            .prepare_build(tree, 10, &crate::steps::stop_requested);
        assert!(stopped.is_err(), "cleanup must stop while the command runs");
        BuildResult::Failed {
            output: "stopped-cleanup.log".into(),
            usage: Usage::default(),
        }
    }
}

#[test]
fn a_rebuild_queued_across_restart_proceeds_when_idle_cleanup_stops() {
    let f = Fixture::new();
    let trees = TreeFixture::with_landing(0, 3, 1);
    let tip = trees.tip();
    let landing = trees.trees.lease_landing("setup", &tip).unwrap().unwrap();
    let spare = trees.trees.lease_landing("setup", &tip).unwrap().unwrap();
    trees.trees.release_landing(&spare.tree, None).unwrap();
    trees
        .trees
        .release_landing(&landing.tree, Some(&tip))
        .unwrap();
    let output = landing.path.join("out/compiled");
    std::fs::create_dir_all(output.parent().unwrap()).unwrap();
    std::fs::write(&output, "over the build ceiling").unwrap();
    let pool = crate::pool::Pool::new(ignite_store::local::open_in_memory().unwrap());
    let processes = Arc::new(Processes::default());
    let runner = Runner(processes.clone());
    let idle = || {
        let mut idle = f.idle();
        idle.host = Box::new(LiveHost {
            trees: Arc::new(trees.trees.clone()),
            repo: trees.repo.clone(),
        });
        idle
    };
    let previous = idle();
    let mut old_ctx = f.ctx.clone();
    old_ctx.git = Arc::new({
        let git = ignite_core::fakes::FakeGit::default();
        git.set_tip("master", tip);
        git
    });
    let (command, running) = mpsc::sync_channel(1);
    old_ctx.builds = Arc::new(CleanupBuild(trees.trees.clone(), command));
    let measured = measured_build(&spare.tree);
    let mut explicit_ctx = old_ctx.clone();
    explicit_ctx.config.rebuild[0].file = "a.txt".to_owned();
    explicit_ctx.builds = measured.clone();
    previous.drive(&old_ctx, &runner, &pool).unwrap();
    let old_handle = Handle::new("I-1").unwrap();
    let job = processes.started.lock().unwrap()[0].1.clone();
    let steps = Steps::new(f.clock.clone());
    let old = steps.begin(Kind::Verb("idle-job".to_owned()), Work::default());
    old.attach_handle(&old_handle);
    let old_progress = old.subscribe();
    let queue = steps.begin(Kind::Verb("rebuild".to_owned()), Work::default());
    let queue_progress = queue.subscribe();
    let explicit = RebuildVerb {
        idle: Arc::new(idle()),
    };
    let (command, stop_cleanup, queue_entered, reply, rebuilt) = thread::scope(|scope| {
        let running_job = scope.spawn(|| {
            let _entered = old.enter();
            previous.run(&old_ctx, &job)
        });
        let command = running.recv_timeout(Duration::from_secs(15));
        let stop_cleanup = wait_line(&old_progress, "shared tree command lock");

        let restarted = idle();
        let settled = restarted.settle(&runner);
        let queued = scope.spawn(|| {
            let _entered = queue.enter();
            explicit.run(&explicit_ctx, &request("rebuild", &["core-body"]))
        });
        let queue_entered = wait_line(&queue_progress, "I-1 to release the idle job lock");

        old.request_stop().unwrap();
        let reply = running_job.join().unwrap();
        let rebuilt = queued.join().unwrap();
        assert!(settled.unwrap());
        (
            command.unwrap(),
            stop_cleanup,
            queue_entered,
            reply,
            rebuilt,
        )
    });

    assert!(stop_cleanup);
    assert!(queue_entered);
    assert!(reply.is_err());
    assert_eq!(rebuilt.exit, Exit::Success, "{}", rebuilt.text);
    assert!(output.exists(), "the live command's outputs were preserved");
    assert!(
        measured
            .requests()
            .iter()
            .all(|(tree, _, _)| tree == &spare.tree)
    );
    processes.finish(Reply::text(Exit::Conflict, "stopped"));
    assert!(f.idle().settle(&runner).is_err());
    assert!(!f.dir.join("state/idle-job.handle").exists());
    let status = trees.trees.status().unwrap();
    assert!(status.trees.iter().all(|tree| tree.state == "free"));
    drop(command);
    let leased_again = trees
        .trees
        .lease_spare_landing("next work", &landing.at)
        .unwrap()
        .unwrap();
    assert_eq!(leased_again.tree, landing.tree);
}

fn measured_build(tree: &TreeId) -> Arc<FakeBuilds> {
    let builds = Arc::new(FakeBuilds::default());
    builds.script(
        tree,
        "build",
        BuildResult::Built {
            usage: Usage::default(),
            units: vec![CompileUnit {
                crate_name: "core".to_owned(),
                mode: "build".to_owned(),
                instructions: Some(100.0),
                cpu_s: 1.0,
                rebuilt: true,
            }],
        },
    );
    builds
}

#[test]
fn stopping_a_rebuild_waiting_for_the_job_lock_leaves_the_owner_running() {
    let f = Fixture::new();
    let owner = f.idle().work().unwrap();
    let steps = Steps::new(f.clock.clone());
    let queue = steps.begin(Kind::Verb("rebuild".to_owned()), Work::default());
    let progress = queue.subscribe();
    let (done, reply) = mpsc::channel();
    let queued = thread::scope(|scope| {
        scope.spawn(|| {
            let _entered = queue.enter();
            let rebuild = RebuildVerb {
                idle: Arc::new(f.idle()),
            };
            done.send(rebuild.run(&f.ctx, &request("rebuild", &["core-body"])))
                .unwrap();
        });
        let saw_lock = wait_line(&progress, "to release the idle job lock");
        queue.request_stop().unwrap();
        let queued = reply.recv_timeout(Duration::from_secs(15));
        drop(owner);
        assert!(saw_lock);
        queued.unwrap()
    });
    assert_eq!(queued.exit, Exit::Conflict);
    assert!(f.host.leased.lock().unwrap().is_empty());
}

fn wait_line(progress: &mpsc::Receiver<String>, text: &str) -> bool {
    while let Ok(line) = progress.recv_timeout(Duration::from_secs(15)) {
        if line.contains(text) {
            return true;
        }
    }
    false
}

#[test]
fn an_idle_job_is_named_by_its_request_arguments() {
    for job in [
        IdleJob::Audit,
        IdleJob::Explore {
            family: "seeds".to_owned(),
        },
        IdleJob::Rebuild {
            name: "core-body".to_owned(),
        },
    ] {
        assert_eq!(IdleJob::from_args(&job.to_args()), Some(job));
    }
    assert_eq!(IdleJob::from_args(&["explore".to_owned()]), None);
}

#[test]
fn the_idle_job_verb_runs_the_job_it_is_asked_for() {
    let f = Fixture::new();
    f.seeds.next.lock().unwrap().extend(super::next_lines(&[
        r#"{"check": "s1", "inputs": ["src/**"]}"#,
    ]));
    let verb = IdleJobVerb {
        idle: Arc::new(f.idle()),
    };

    let reply = verb.run(&f.ctx, &request("idle-job", &["explore", "seeds"]));

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert_eq!(f.host.leased.lock().unwrap().len(), 1);
    let nonsense = verb.run(&f.ctx, &request("idle-job", &["nothing"]));
    assert_eq!(nonsense.exit, Exit::Usage);
}

#[test]
fn processes_sharing_the_state_file_lose_none_of_each_others_changes() {
    let f = Fixture::new();
    // The daemon's idle work, loaded before the idle job's process changes the file.
    let daemon = f.idle();
    let job = f.idle();
    let batch = NewCheck {
        check: super::check("seeds:s1"),
        digest: None,
        inputs: Vec::new(),
    };
    job.update(|state| state.batches.insert("seeds".to_owned(), vec![batch]))
        .unwrap();

    // The daemon changes the file from what it read first; the job's change must survive.
    daemon
        .update(|state| {
            state
                .spent
                .insert("seeds".to_owned(), std::collections::BTreeMap::default())
        })
        .unwrap();

    let saved = super::State::load(&f.dir.join("state/idle.json")).unwrap();
    assert!(
        saved.batches.contains_key("seeds"),
        "the job's batch was lost"
    );
    assert!(saved.spent.contains_key("seeds"));
    // And what the daemon decides from is the file, not what it read at start.
    assert_eq!(
        daemon.next_job(&f.ctx),
        Some(IdleJob::Explore {
            family: "seeds".to_owned()
        })
    );
}

/// Landing trees in a verb process are leased through the daemon for the idle job's handle,
/// leaving one free.
#[test]
fn a_verb_process_leases_idle_trees_through_the_daemon_leaving_one_free() {
    struct Local;
    impl IdleHost for Local {
        fn lease(&self, _: &Commit) -> Result<Option<(TreeId, std::path::PathBuf)>> {
            panic!("a verb process leases through the daemon");
        }
        fn release(&self, _: &TreeId, _: Option<&Commit>) -> Result<()> {
            panic!("a verb process releases through the daemon");
        }
        fn commits_between(&self, _: &Commit, _: &Commit) -> Result<usize> {
            Ok(7)
        }
    }
    let f = Fixture::new();
    let handle = Handle::new("I-4").unwrap();
    let host = Arc::new(FakeHost::default());
    host.script_lease(LeaseLandingReply::Leased {
        tree: TreeId::new("landing-2").unwrap(),
        path: f.dir.join("tree"),
        branch: "landing-2".to_owned(),
        at: commit("aaaaaaa"),
    });
    let link = Arc::new(Link::new(
        host.clone(),
        Hello {
            handle: handle.clone(),
            protocol: PROTOCOL_VERSION,
            executable: "0000000".to_owned(),
            pid: 1,
        },
        Timing::default(),
    ));
    let remote = Arc::new(Remote {
        link,
        ids: Arc::new(RequestIds::new(handle.clone())),
        clock: f.clock.clone(),
        notes: f.dir.clone(),
    });
    let idle_host = RemoteIdleHost {
        remote,
        local: Box::new(Local),
    };

    let (tree, path) = idle_host.lease(&commit("aaaaaaa")).unwrap().unwrap();
    idle_host.release(&tree, Some(&commit("aaaaaaa"))).unwrap();

    assert_eq!(tree.as_str(), "landing-2");
    assert_eq!(path, f.dir.join("tree"));
    let calls = host.calls();
    let lease = calls
        .iter()
        .find(|(verb, _)| verb == "host-lease-landing")
        .expect("the tree was leased through the daemon");
    assert_eq!(lease.1["keep_free"], 1);
    assert_eq!(lease.1["handle"], "I-4");
    assert_eq!(host.released().len(), 1);
    assert_eq!(
        idle_host
            .commits_between(&commit("aaaa"), &commit("bbbb"))
            .unwrap(),
        7
    );
}

#[test]
fn an_off_machine_settles_its_running_idle_job_and_resumes_on_an_active_notice() {
    use ignite_core::fleet::{FleetNotice, MachineState};

    let f = Fixture::new();
    let idle = f.idle();
    let processes = Arc::new(Processes::default());
    let runner = Runner(processes.clone());
    let pool = crate::pool::Pool::new(ignite_store::local::open_in_memory().unwrap());
    idle.drive(&f.ctx, &runner, &pool).unwrap();
    assert_eq!(processes.starts(), 1);

    let mut notice = FleetNotice {
        version: 1,
        state: MachineState::Off,
        ..FleetNotice::default()
    };
    crate::fleet_notice::store(&pool, &notice).unwrap();
    processes.finish(Reply::text(Exit::Success, "done"));
    idle.drive(&f.ctx, &runner, &pool).unwrap();
    idle.drive(&f.ctx, &runner, &pool).unwrap();
    assert_eq!(processes.starts(), 1);
    assert!(!f.dir.join("state/idle-job.handle").exists());

    notice.state = MachineState::Active;
    notice.version += 1;
    crate::fleet_notice::store(&pool, &notice).unwrap();
    idle.drive(&f.ctx, &runner, &pool).unwrap();
    assert_eq!(processes.starts(), 2);
}
