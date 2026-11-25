use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::*;
use ignite_core::fakes::FakeLauncher;
use ignite_core::protocol::{Class, Usage};
use ignite_core::time::SystemClock;

use crate::steps::{Kind, Steps, Work};

fn spec(args: &[&str]) -> CommandSpec {
    CommandSpec {
        args: args.iter().map(|s| (*s).to_owned()).collect(),
        cwd: PathBuf::from("/repo/trees/tree-01"),
        env: Vec::new(),
        class: Class::Agent,
        cores: 1,
        resources: Vec::new(),
        grows: false,
        tree: None,
        ticket: None,
        hang_limit_s: None,
        reserve: None,
    }
}

fn result(exit: i32, output: &str) -> RunResult {
    RunResult {
        exit: Some(exit),
        output: PathBuf::from(output),
        usage: Usage::default(),
        hung: false,
    }
}

fn handle(name: &str) -> Handle {
    Handle::new(name).unwrap()
}

/// A command a previous daemon started for `owner`, whose wait returns `exit` and counts.
fn previous(owner: &str, args: &[&str], exit: i32, waits: &Arc<AtomicUsize>) -> Previous {
    let waits = waits.clone();
    Previous {
        handle: handle(owner),
        args: spec(args).args,
        cwd: spec(args).cwd,
        env: Vec::new(),
        wait: Box::new(move || {
            waits.fetch_add(1, Ordering::SeqCst);
            result(exit, "/runs/previous/output.log")
        }),
        end: Box::new(|| {}),
    }
}

/// Runs `body` inside an activity whose reply goes to `owner`, if given.
fn as_handle<T>(owner: Option<&str>, body: impl FnOnce() -> T) -> T {
    let steps = Steps::new(Arc::new(SystemClock));
    let activity = steps.begin(Kind::Verb("test".to_owned()), Work::default());
    if let Some(owner) = owner {
        activity.attach_handle(&handle(owner));
    }
    let _entered = activity.enter();
    body()
}

#[test]
fn the_verb_run_again_under_a_handle_takes_over_its_running_command_once() {
    let inner = Arc::new(FakeLauncher::default());
    inner.script(&["cargo"], result(0, "/runs/new/output.log"));
    let waits = Arc::new(AtomicUsize::new(0));
    let launcher = AdoptingLauncher::new(
        inner.clone(),
        vec![previous("T-4", &["cargo", "nextest", "run"], 2, &waits)],
    );

    let adopted = as_handle(Some("T-4"), || {
        launcher.run(&spec(&["cargo", "nextest", "run"]))
    });
    // The previous run's result, and nothing started again.
    assert_eq!(adopted.exit, Some(2));
    assert_eq!(waits.load(Ordering::SeqCst), 1);
    assert!(inner.runs().is_empty());
    assert_eq!(launcher.waiting(), 0);

    // A second run of the same command starts it: the previous one was taken.
    let again = as_handle(Some("T-4"), || {
        launcher.run(&spec(&["cargo", "nextest", "run"]))
    });
    assert_eq!(again.exit, Some(0));
    assert_eq!(inner.runs().len(), 1);
}

#[test]
fn another_handle_a_loop_or_another_command_is_never_adopted() {
    let inner = Arc::new(FakeLauncher::default());
    inner.script(&["cargo"], result(0, "/runs/new/output.log"));
    let waits = Arc::new(AtomicUsize::new(0));
    let launcher = AdoptingLauncher::new(
        inner.clone(),
        vec![previous("T-4", &["cargo", "nextest", "run"], 2, &waits)],
    );
    as_handle(Some("T-5"), || {
        launcher.run(&spec(&["cargo", "nextest", "run"]))
    });
    as_handle(None, || launcher.run(&spec(&["cargo", "nextest", "run"])));
    as_handle(Some("T-4"), || launcher.run(&spec(&["cargo", "build"])));
    assert_eq!(waits.load(Ordering::SeqCst), 0);
    assert_eq!(inner.runs().len(), 3);
    assert_eq!(launcher.waiting(), 1);
}

#[test]
fn a_command_started_for_a_handle_carries_it_in_its_environment() {
    let inner = Arc::new(FakeLauncher::default());
    inner.script(&["cargo"], result(0, "/runs/new/output.log"));
    let launcher = AdoptingLauncher::new(inner.clone(), Vec::new());
    as_handle(Some("L-7"), || launcher.run(&spec(&["cargo", "build"])));
    as_handle(None, || launcher.run(&spec(&["cargo", "build"])));
    let runs = inner.runs();
    assert_eq!(
        runs[0].env,
        [(HANDLE_VARIABLE.to_owned(), "L-7".to_owned())]
    );
    assert!(runs[1].env.is_empty());
}

#[test]
fn an_adopted_command_is_ended_when_its_caller_cancels() {
    let ended = Arc::new(AtomicBool::new(false));
    let (release, finished) = std::sync::mpsc::channel::<()>();
    let flag = ended.clone();
    let previous = Previous {
        handle: handle("T-4"),
        args: spec(&["cargo", "test"]).args,
        cwd: spec(&["cargo", "test"]).cwd,
        env: Vec::new(),
        wait: Box::new(move || {
            let _ = finished.recv();
            RunResult {
                exit: None,
                output: PathBuf::from("/runs/previous/output.log"),
                usage: Usage::default(),
                hung: false,
            }
        }),
        end: Box::new(move || {
            flag.store(true, Ordering::SeqCst);
            let _ = release.send(());
        }),
    };
    let launcher = AdoptingLauncher::new(Arc::new(FakeLauncher::default()), vec![previous]);
    let result = as_handle(Some("T-4"), || {
        launcher.run_until(&spec(&["cargo", "test"]), &|| true)
    });
    assert!(ended.load(Ordering::SeqCst));
    assert_eq!(result.exit, None);
}

#[test]
fn files_an_adopted_command_wrote_reach_the_paths_the_second_run_named() {
    let dir = ignite_core::test_files::temp_dir()
        .join(format!("ignite-adopt-carry-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (first, second) = (dir.join("results-1.jsonl"), dir.join("results-2.jsonl"));
    let written = first.clone();
    let previous = Previous {
        handle: handle("T-4"),
        args: spec(&["sh", "adapter.sh", "run"]).args,
        cwd: spec(&["sh", "adapter.sh", "run"]).cwd,
        env: vec![
            ("IGNITE_RESULTS".to_owned(), first.display().to_string()),
            ("GREETING".to_owned(), "hello".to_owned()),
        ],
        wait: Box::new(move || {
            std::fs::write(&written, "{\"check\": \"core:a\", \"outcome\": \"pass\"}\n").unwrap();
            result(0, "/runs/previous/output.log")
        }),
        end: Box::new(|| {}),
    };
    let launcher = AdoptingLauncher::new(Arc::new(FakeLauncher::default()), vec![previous]);
    let mut asked = spec(&["sh", "adapter.sh", "run"]);
    asked.env = vec![
        ("IGNITE_RESULTS".to_owned(), second.display().to_string()),
        ("GREETING".to_owned(), "hi".to_owned()),
    ];
    as_handle(Some("T-4"), || launcher.run(&asked));
    assert_eq!(
        std::fs::read_to_string(&second).unwrap(),
        "{\"check\": \"core:a\", \"outcome\": \"pass\"}\n"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A previous command is taken over only at the commit it started at: one that recorded a
/// commit the checkout no longer has (here, a directory outside every checkout) is ended and
/// started again.
#[test]
fn a_command_that_started_at_another_commit_is_ended_not_adopted() {
    let inner = Arc::new(FakeLauncher::default());
    inner.script(&["cargo"], result(0, "/runs/new/output.log"));
    let waits = Arc::new(AtomicUsize::new(0));
    let ended = Arc::new(AtomicBool::new(false));
    let mut stale = previous("L-4", &["cargo", "nextest", "run"], 2, &waits);
    stale.env = vec![(
        TREE_COMMIT_VARIABLE.to_owned(),
        "1234567890abcdef1234567890abcdef12345678".to_owned(),
    )];
    let flag = ended.clone();
    stale.end = Box::new(move || flag.store(true, Ordering::SeqCst));
    let launcher = AdoptingLauncher::new(inner.clone(), vec![stale]);

    let result = as_handle(Some("L-4"), || {
        launcher.run(&spec(&["cargo", "nextest", "run"]))
    });
    assert_eq!(result.exit, Some(0));
    assert_eq!(waits.load(Ordering::SeqCst), 0);
    assert!(ended.load(Ordering::SeqCst));
    assert_eq!(inner.runs().len(), 1);
    assert_eq!(launcher.waiting(), 0);
}

/// A restart once ended a `cargo nextest run` that was compiling in a landing tree, and every
/// later build there failed to link the crate it had been compiling. Ending such a run now marks
/// its target directory, so the next build there clears what the compile left unfinished.
#[test]
fn ending_a_compiling_run_in_a_reset_tree_marks_its_target_directory() {
    let dir = ignite_core::test_files::temp_dir()
        .join(format!("ignite-adopt-{}-ended", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let tree = dir.join("trees").join("landing-2");
    let target = tree.join("rust").join("target");
    std::fs::create_dir_all(&target).unwrap();
    let runs = dir.join("runs");
    let started = |name: &str, args: &[&str], cwd: &Path| {
        let run = runs.join(name);
        std::fs::create_dir_all(&run).unwrap();
        let spec = SpecFile {
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            tree_path: None,
            cwd: cwd.to_path_buf(),
            env: Vec::new(),
            queued_s: 0.0,
            jobserver: None,
            build_jobs: None,
            hang_limit_s: None,
            grant: None,
        };
        std::fs::write(run.join("spec.json"), serde_json::to_vec(&spec).unwrap()).unwrap();
    };
    started("1-1-0", &["cargo", "nextest", "run"], &tree.join("rust"));
    started("1-1-1", &["cargo", "nextest", "run"], &dir);

    let unfinished = SupervisedLauncher::new(Path::new("ignite"), runs).unfinished();
    let kept = end_runs_in(unfinished, std::slice::from_ref(&tree));
    assert_eq!(kept.len(), 1, "the run outside the reset tree is kept");
    assert!(target.join(crate::build::repair::SUSPECT_FILE).is_file());
    std::fs::remove_dir_all(&dir).unwrap();
}

struct IdleHost;

impl ignite_os::hostcpu::HostCpu for IdleHost {
    fn cores(&self) -> u32 {
        8
    }

    fn busy(&self) -> std::io::Result<f64> {
        Ok(0.0)
    }

    fn free_memory_mb(&self) -> std::io::Result<u64> {
        Err(std::io::Error::other("not read"))
    }
}

/// On cygnus a `cargo nextest run` left by a landing whose verb process had died kept its
/// `land` grant through two daemon restarts, so every core read as granted while the machine
/// idled. After a restart, a previous command whose handle has ended is ended, and its grant is
/// released when its supervisor exits, while the command of a handle that still runs keeps its
/// grant and is handed to that handle's verb.
#[test]
fn a_restart_ends_the_command_of_a_dead_verb_and_keeps_the_live_ones() {
    let scheduler = Arc::new(crate::sched::Scheduler::new(
        crate::sched::Limits {
            cores: 8,
            reserved: 0,
            tip_build: 0,
            floor: ignite_core::capacity::Floor::default(),
            resources: std::collections::BTreeMap::new(),
        },
        Arc::new(ignite_core::fakes::FakeClock::new(
            ignite_core::time::Timestamp::from_millis(1_800_000_000_000),
        )),
        Arc::new(IdleHost),
    ));
    // As `SupervisedLauncher::adopt_unfinished` does at start: each command's grant is adopted
    // and released when its supervisor exits, which the fakes do when the command is ended.
    let held = |owner: &str, args: &[&str], ended: &Arc<AtomicBool>| {
        let grant = scheduler.adopt(Class::Land, 3, Vec::new(), None, None);
        let (scheduler, flag) = (scheduler.clone(), ended.clone());
        let waits = Arc::new(AtomicUsize::new(0));
        let mut run = previous(owner, args, 0, &waits);
        run.end = Box::new(move || {
            flag.store(true, Ordering::SeqCst);
            scheduler.release(&grant);
        });
        run
    };
    let (dead_ended, live_ended) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    let handles = crate::handles::Handles::new(Arc::new(SystemClock));
    let dead = handles.create("land", None, None);
    let live = handles.create("land", None, None);
    let launcher = AdoptingLauncher::new(
        Arc::new(FakeLauncher::default()),
        vec![
            held(
                dead.as_str(),
                &["cargo", "nextest", "run", "--no-run"],
                &dead_ended,
            ),
            held(live.as_str(), &["cargo", "nextest", "run"], &live_ended),
        ],
    );
    assert_eq!(scheduler.status().granted, 6);

    // The restarted daemon runs `live` again; `dead`'s verb could not be run again and ended
    // `incomplete`.
    handles.finish_incomplete(
        &dead,
        crate::handles::incomplete("land", &dead, "its verb process died"),
    );
    let running = |owner: &Handle| handles.is_running(owner);
    assert_eq!(launcher.end_orphans(&running), 1);
    assert!(dead_ended.load(Ordering::SeqCst));
    assert!(!live_ended.load(Ordering::SeqCst));
    assert_eq!(scheduler.status().granted, 3, "the dead verb's cores are free");
    assert_eq!(launcher.waiting(), 1);

    // Sweeping again ends nothing more, and the live verb still takes its command over.
    assert_eq!(launcher.end_orphans(&running), 0);
    let taken = as_handle(Some(live.as_str()), || {
        launcher.run(&spec(&["cargo", "nextest", "run"]))
    });
    assert_eq!(taken.exit, Some(0));
    assert!(!live_ended.load(Ordering::SeqCst));
    assert_eq!(launcher.waiting(), 0);
}

#[test]
fn a_handle_runs_until_it_finishes_and_an_unknown_one_never_runs() {
    let handles = crate::handles::Handles::new(Arc::new(SystemClock));
    let land = handles.create("land", None, None);
    assert!(handles.is_running(&land));
    handles.finish(
        &land,
        crate::handles::incomplete("land", &land, "finished for the test"),
    );
    assert!(!handles.is_running(&land));
    assert!(!handles.is_running(&handle("L-99999")));
}
