//! The idle work against the fakes: exploration never replays a check, audits report
//! disagreements with both runs, rebuilds leave the tree's files as they were, and each of the
//! three gives the same result when it is stopped midway and started again.

use std::collections::BTreeMap;
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ignite_core::config::{Config, Os};
use ignite_core::fakes::{FakeClock, FakeGit, FakeHub};
use ignite_core::ids::{CheckName, Commit, InputHash, MachineId, RecordId, TreeId};
use ignite_core::manifest::Manifest;
use ignite_core::protocol::client::{Exit, Request};
use ignite_core::protocol::hub::{CachedResult, CompileUnit, ResultKey};
use ignite_core::protocol::{Outcome, Usage};
use ignite_core::time::Timestamp;
use ignite_core::traits::{BuildResult, Builds, CommandSpec, Error, Launcher, Result, RunResult};

use super::rebuild::RebuildCount;
use super::state::State;
use super::{AuditVerb, Idle, IdleHost, IdleJob, IdleSettings, RebuildVerb, Reran, Rerun};
use crate::checks::HitSink;
use crate::server::{Context, Verb};
use crate::testhub::SpyHub;
use crate::trees::DiskReport;

#[path = "backoff_tests.rs"]
mod backoff;
#[path = "disk_tests.rs"]
mod disk;
#[path = "launch_tests.rs"]
mod launch;

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

fn temp_dir() -> PathBuf {
    let n = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("ignite-idle-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

const CONFIG: &str = r#"
[repository]
remote = "origin"
staging = "staging"
master = "master"
trees = "trees"
checks = "checks"
[hub]
url = "http://localhost:1"
project = "test"
[gate]
threshold = 0.01
cap = 10
[machine.local]
ticket_trees = 1
landing_trees = 1

[[rebuild]]
name = "core-body"
edit = "append a no-op item to the core crate"
file = "src/core.rs"
append = "\nconst _: () = ();\n"
products = ["build"]
"#;

const EXPLORE: &str = r#"
[family.seeds]
kind = "exploration"
adapter = "jsonl"
platforms = "any"
list = "seeds list"
next = "seeds next"
run = "seeds run {checks}"
inputs = ["src/**"]
"#;

fn commit(text: &str) -> Commit {
    Commit::new(text).unwrap()
}

fn check(name: &str) -> CheckName {
    CheckName::new(name).unwrap()
}

/// A landing tree in a temporary directory, and the history between commits.
struct Host {
    dir: PathBuf,
    busy: AtomicBool,
    between: AtomicUsize,
    /// How many times the disk was held.
    held: AtomicUsize,
    /// The bytes short of the free space kept, as the last hold was told.
    short: AtomicU64,
    /// How many times free trees were cleared to bring the disk back to its floor.
    freed: AtomicUsize,
    /// The floor in gigabytes, as the last clearing was told.
    floor: AtomicU64,
    /// Gigabytes free on the trees' disk; `None` reads as unknown.
    free_gb: Mutex<Option<u64>>,
    leased: Mutex<Vec<Commit>>,
    released: Mutex<Vec<(TreeId, Option<Commit>)>>,
}

impl IdleHost for Arc<Host> {
    fn lease(&self, commit: &Commit) -> Result<Option<(TreeId, PathBuf)>> {
        if self.busy.load(Ordering::SeqCst) {
            return Ok(None);
        }
        self.leased.lock().unwrap().push(commit.clone());
        Ok(Some((TreeId::new("landing-1").unwrap(), self.dir.clone())))
    }

    fn release(&self, tree: &TreeId, built_at: Option<&Commit>) -> Result<()> {
        self.released
            .lock()
            .unwrap()
            .push((tree.clone(), built_at.cloned()));
        Ok(())
    }

    fn commits_between(&self, _from: &Commit, _to: &Commit) -> Result<usize> {
        Ok(self.between.load(Ordering::SeqCst))
    }

    fn hold_disk(&self, short: u64) -> Result<DiskReport> {
        self.held.fetch_add(1, Ordering::SeqCst);
        self.short.store(short, Ordering::SeqCst);
        Ok(DiskReport::default())
    }

    fn free_disk(&self, floor_gb: u64) -> Result<DiskReport> {
        self.freed.fetch_add(1, Ordering::SeqCst);
        self.floor.store(floor_gb, Ordering::SeqCst);
        Ok(DiskReport::default())
    }

    fn disk_free_gb(&self) -> Option<u64> {
        *self.free_gb.lock().unwrap()
    }
}

/// `seeds next` prints the scripted lines; `seeds run` writes each named check's scripted
/// outcome to `IGNITE_RESULTS`, or panics, as a stop would end it, while `stop` is set.
#[derive(Default)]
struct Seeds {
    dir: PathBuf,
    next: Mutex<Vec<String>>,
    outcomes: Mutex<BTreeMap<String, Outcome>>,
    stop: AtomicBool,
    runs: Mutex<Vec<Vec<String>>>,
}

impl Launcher for Seeds {
    fn run(&self, spec: &CommandSpec) -> RunResult {
        self.runs.lock().unwrap().push(spec.args.clone());
        let output = self
            .dir
            .join(format!("out-{}", self.runs.lock().unwrap().len()));
        match spec.args.get(1).map(String::as_str) {
            Some("next") => fs::write(&output, self.next.lock().unwrap().join("\n")).unwrap(),
            Some("run") => {
                assert!(!self.stop.load(Ordering::SeqCst), "stopped");
                let (_, file) = spec
                    .env
                    .iter()
                    .find(|(k, _)| k == "IGNITE_RESULTS")
                    .unwrap();
                let outcomes = self.outcomes.lock().unwrap();
                let lines: Vec<String> = spec.args[2..]
                    .iter()
                    .map(|name| {
                        let outcome = outcomes.get(name).copied().unwrap_or(Outcome::Pass);
                        let word = if outcome == Outcome::Fail {
                            "fail"
                        } else {
                            "pass"
                        };
                        format!(r#"{{"check": "{name}", "outcome": "{word}"}}"#)
                    })
                    .collect();
                fs::write(file, lines.join("\n")).unwrap();
                fs::write(&output, "seed output\n").unwrap();
            }
            _ => fs::write(&output, "").unwrap(),
        }
        RunResult {
            exit: Some(0),
            output,
            usage: Usage {
                instructions: Some(2_000_000_000),
                ..Usage::default()
            },
            hung: false,
        }
    }
}

/// Builds that report one compiler unit per crate the edit touches, and panic, as a stop would
/// end them, on the build after the edit while `stop` is set.
#[derive(Default)]
struct Compiler {
    dir: PathBuf,
    stop: AtomicBool,
    /// Every build fails, as a broken commit makes it.
    broken: AtomicBool,
}

impl Builds for Compiler {
    fn ensure(&self, _tree: &TreeId, _product: &str, _packages: &[String]) -> BuildResult {
        if self.broken.load(Ordering::SeqCst) {
            return BuildResult::Failed {
                output: self.dir.join("build.log"),
                usage: Usage::default(),
            };
        }
        let edited = fs::read_to_string(self.dir.join("src/core.rs"))
            .unwrap()
            .contains("const _");
        if !edited {
            return BuildResult::Current;
        }
        assert!(!self.stop.load(Ordering::SeqCst), "stopped");
        let unit = |name: &str, instructions: f64| CompileUnit {
            crate_name: name.to_owned(),
            mode: "build".to_owned(),
            instructions: Some(instructions),
            cpu_s: 1.0,
            rebuilt: true,
        };
        BuildResult::Built {
            usage: Usage::default(),
            units: vec![unit("core", 30e9), unit("app", 12e9), unit("core", 3e9)],
        }
    }
}

/// Reruns that answer from a script, or panic while `stop` is set.
#[derive(Default)]
struct Reruns {
    outcomes: Mutex<BTreeMap<String, Outcome>>,
    stop: AtomicBool,
    runs: Mutex<Vec<CheckName>>,
}

impl Rerun for Reruns {
    fn rerun(&self, _ctx: &Context, _tree: &TreeId, check: &CheckName) -> Reran {
        assert!(!self.stop.load(Ordering::SeqCst), "stopped");
        self.runs.lock().unwrap().push(check.clone());
        let outcome = self.outcomes.lock().unwrap().get(check.as_str()).copied();
        let outcome = outcome.unwrap_or(Outcome::Pass);
        (
            outcome,
            (outcome == Outcome::Fail).then(|| "assertion failed".to_owned()),
            Some(RecordId(91)),
        )
    }
}

struct Fixture {
    dir: PathBuf,
    host: Arc<Host>,
    seeds: Arc<Seeds>,
    compiler: Arc<Compiler>,
    reruns: Arc<Reruns>,
    hub: Arc<FakeHub>,
    clock: Arc<FakeClock>,
    ctx: Context,
}

impl Fixture {
    fn new() -> Fixture {
        let dir = temp_dir();
        let tree = dir.join("tree");
        fs::create_dir_all(tree.join("src")).unwrap();
        fs::write(tree.join("src/core.rs"), "pub fn core() {}\n").unwrap();
        fs::write(tree.join("src/other.rs"), "pub fn other() {}\n").unwrap();
        let host = Arc::new(Host {
            dir: tree.clone(),
            busy: AtomicBool::new(false),
            between: AtomicUsize::new(0),
            held: AtomicUsize::new(0),
            short: AtomicU64::new(0),
            freed: AtomicUsize::new(0),
            floor: AtomicU64::new(0),
            free_gb: Mutex::new(None),
            leased: Mutex::new(Vec::new()),
            released: Mutex::new(Vec::new()),
        });
        let seeds = Arc::new(Seeds {
            dir: dir.clone(),
            ..Seeds::default()
        });
        let compiler = Arc::new(Compiler {
            dir: tree,
            ..Compiler::default()
        });
        let hub = Arc::new(FakeHub::default());
        let clock = Arc::new(FakeClock::new(
            Timestamp::parse("2026-11-18T10:00:00Z").unwrap(),
        ));
        let git = FakeGit::default();
        git.set_tip("master", commit("aaaaaaa"));
        let ctx = Context {
            config: Config::parse("ignite.toml", CONFIG).unwrap(),
            repo: dir.clone(),
            machine: MachineId::new("m-1").unwrap(),
            clock: clock.clone(),
            hub: hub.clone(),
            git: Arc::new(git),
            launcher: seeds.clone(),
            builds: compiler.clone(),
            session: None,
            tree: None,
        };
        Fixture {
            dir,
            host,
            seeds,
            compiler,
            reruns: Arc::new(Reruns::default()),
            hub,
            clock,
            ctx,
        }
    }

    /// The idle work as a daemon that has just started would build it, from the state file.
    fn idle(&self) -> Idle {
        self.idle_on(Os::Macos)
    }

    fn idle_on(&self, os: Os) -> Idle {
        let manifest =
            Manifest::from_files(&[(PathBuf::from("checks/seeds.toml"), EXPLORE.to_owned())])
                .unwrap();
        let settings = IdleSettings {
            manifest: manifest.into(),
            os,
            families: None,
            state: self.dir.join("state/idle.json"),
            outputs: self.dir.join("outputs"),
            hostname: "host-1".to_owned(),
            cores: 2,
            ignite: "0000000".to_owned(),
        };
        Idle::new(
            settings,
            self.clock.clone(),
            Box::new(self.host.clone()),
            self.reruns.clone(),
        )
        .unwrap()
    }

    fn tree_file(&self, path: &str) -> String {
        fs::read_to_string(self.host.dir.join(path)).unwrap()
    }

    fn seed_runs(&self) -> Vec<Vec<String>> {
        self.seeds.runs.lock().unwrap().clone()
    }
}

fn request(verb: &str, args: &[&str]) -> Request {
    Request {
        verb: verb.to_owned(),
        args: args.iter().map(|s| (*s).to_owned()).collect(),
        cwd: PathBuf::from("/repo"),
        session: None,
        wait_s: 1,
        pid: None,
    }
}

fn next_lines(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|s| (*s).to_owned()).collect()
}

/// Runs `job`, which panics partway as a stop would end it.
fn stopped(idle: &Idle, ctx: &Context, job: &IdleJob) {
    let result = catch_unwind(AssertUnwindSafe(|| idle.run(ctx, job)));
    assert!(result.is_err(), "the job should have been stopped");
}

#[test]
fn an_explored_check_is_never_asked_for_again_on_the_same_inputs() {
    let f = Fixture::new();
    let idle = f.idle();
    *f.seeds.next.lock().unwrap() = next_lines(&[
        r#"{"check": "seeds:s1", "digest": "d1"}"#,
        "progress: reserved 2 seeds",
        r#"{"check": "seeds:s2", "digest": "d2"}"#,
    ]);
    f.seeds
        .outcomes
        .lock()
        .unwrap()
        .insert("seeds:s2".to_owned(), Outcome::Fail);
    let job = idle.next_job(&f.ctx).unwrap();
    assert_eq!(
        job,
        IdleJob::Rebuild {
            name: "core-body".to_owned()
        }
    );
    idle.run(&f.ctx, &job).unwrap();
    let job = idle.next_job(&f.ctx).unwrap();
    assert_eq!(
        job,
        IdleJob::Explore {
            family: "seeds".to_owned()
        }
    );
    idle.run(&f.ctx, &job).unwrap();
    let runs = f.seed_runs();
    assert_eq!(runs[1], ["seeds", "run", "seeds:s1", "seeds:s2"]);
    let finds = f.hub.exploration_finds();
    assert_eq!(finds.len(), 1);
    assert_eq!(finds[0].check, check("seeds:s2"));
    assert_eq!(finds[0].replay, "seeds run seeds:s2");
    assert_eq!(finds[0].host, "host-1");
    assert_eq!(finds[0].commit, commit("aaaaaaa"));
    assert!(
        finds[0].output.contains("seed output"),
        "{}",
        finds[0].output
    );

    // The command prints s2 again on the same inputs, and s1 on new ones.
    *f.seeds.next.lock().unwrap() = next_lines(&[
        r#"{"check": "seeds:s2", "digest": "d2"}"#,
        r#"{"check": "seeds:s1", "digest": "d9"}"#,
    ]);
    let restarted = f.idle();
    restarted.run(&f.ctx, &job).unwrap();
    assert_eq!(f.seed_runs()[3], ["seeds", "run", "seeds:s1"]);
    // Nothing new: `next` runs, nothing else does.
    restarted.run(&f.ctx, &job).unwrap();
    assert_eq!(f.seed_runs().len(), 5);
    assert_eq!(f.hub.exploration_finds().len(), 1);
}

#[test]
fn an_exploration_stopped_midway_runs_the_same_batch_when_started_again() {
    let f = Fixture::new();
    let job = IdleJob::Explore {
        family: "seeds".to_owned(),
    };
    *f.seeds.next.lock().unwrap() = next_lines(&[r#"{"check": "seeds:s1", "digest": "d1"}"#]);
    f.seeds
        .outcomes
        .lock()
        .unwrap()
        .insert("seeds:s1".to_owned(), Outcome::Fail);
    f.seeds.stop.store(true, Ordering::SeqCst);
    stopped(&f.idle(), &f.ctx, &job);
    assert!(f.hub.exploration_finds().is_empty());

    // `next` never prints s1 again, so the batch it printed before the stop must be run.
    f.seeds.stop.store(false, Ordering::SeqCst);
    f.seeds.next.lock().unwrap().clear();
    let restarted = f.idle();
    assert_eq!(restarted.next_job(&f.ctx), Some(job.clone()));
    restarted.run(&f.ctx, &job).unwrap();
    let runs = f.seed_runs();
    assert_eq!(runs.len(), 3, "{runs:?}");
    assert_eq!(runs[2], ["seeds", "run", "seeds:s1"]);
    assert_eq!(f.hub.exploration_finds().len(), 1);
}

#[test]
fn exploration_reports_wait_for_the_hub() {
    let f = Fixture::new();
    let job = IdleJob::Explore {
        family: "seeds".to_owned(),
    };
    *f.seeds.next.lock().unwrap() = next_lines(&[r#"{"check": "seeds:s1"}"#]);
    f.seeds
        .outcomes
        .lock()
        .unwrap()
        .insert("seeds:s1".to_owned(), Outcome::Fail);
    f.hub.set_unreachable(true);
    f.idle().run(&f.ctx, &job).unwrap();
    f.hub.set_unreachable(false);
    f.seeds.next.lock().unwrap().clear();
    f.idle().run(&f.ctx, &job).unwrap();
    assert_eq!(f.hub.exploration_finds().len(), 1);
}

#[test]
fn a_state_file_that_cannot_be_read_starts_the_idle_work_over() {
    let f = Fixture::new();
    let state = f.dir.join("state/idle.json");
    fs::create_dir_all(state.parent().unwrap()).unwrap();
    fs::write(&state, "{\"played\": {\"seeds\": [").unwrap();
    // Building the idle work does not fail, so the daemon starts.
    let idle = f.idle();
    assert_eq!(*idle.state(), State::default());
    assert!(
        state.with_file_name("idle.json.unreadable").exists(),
        "the unreadable file is kept aside"
    );
    // An empty file is as unreadable as a cut-off one.
    fs::write(&state, "").unwrap();
    f.idle();
}

#[test]
fn a_report_the_hub_rejects_is_dropped_and_the_next_one_is_sent() {
    let f = Fixture::new();
    let job = IdleJob::Explore {
        family: "seeds".to_owned(),
    };
    *f.seeds.next.lock().unwrap() =
        next_lines(&[r#"{"check": "seeds:s1"}"#, r#"{"check": "seeds:s2"}"#]);
    for check in ["seeds:s1", "seeds:s2"] {
        f.seeds
            .outcomes
            .lock()
            .unwrap()
            .insert(check.to_owned(), Outcome::Fail);
    }
    f.hub.set_unreachable(true);
    let idle = f.idle();
    idle.run(&f.ctx, &job).unwrap();
    assert_eq!(idle.state().unsent.len(), 2);
    f.hub.set_unreachable(false);

    // The hub finds the first report malformed and takes the second.
    let sent = Arc::new(AtomicUsize::new(0));
    let hub = {
        let sent = sent.clone();
        SpyHub::new(f.hub.clone()).refusing(move |name| {
            (name == "find" && sent.fetch_add(1, Ordering::SeqCst) == 0)
                .then(|| Error::Invalid("422 not a find".to_owned()))
        })
    };
    let mut ctx = f.ctx.clone();
    ctx.hub = Arc::new(hub);
    f.seeds.next.lock().unwrap().clear();
    idle.run(&ctx, &job).unwrap();
    assert!(idle.state().unsent.is_empty());
    assert_eq!(f.hub.exploration_finds().len(), 1);
}

fn cached(name: &str, outcome: Outcome) -> CachedResult {
    CachedResult {
        key: ResultKey {
            check: check(name),
            hash: InputHash::new("b".repeat(64)).unwrap(),
            platform: None,
        },
        outcome,
        machine: MachineId::new("m-2").unwrap(),
        record: RecordId(77),
        at: Timestamp::EPOCH,
        output: None,
    }
}

#[test]
fn an_audit_that_disagrees_is_reported_with_both_runs() {
    let f = Fixture::new();
    let idle = Arc::new(f.idle());
    idle.hit(
        &cached("rust:a::flaky_input", Outcome::Pass),
        &commit("bbbbbbb"),
    );
    f.reruns
        .outcomes
        .lock()
        .unwrap()
        .insert("rust:a::flaky_input".to_owned(), Outcome::Fail);
    let verb = AuditVerb { idle: idle.clone() };
    let reply = verb.run(&f.ctx, &request("audit", &["--now"]));
    assert_eq!(reply.exit, Exit::Success);
    assert!(reply.text.contains("1 disagreed"), "{}", reply.text);
    let reports = f.hub.audit_reports();
    assert_eq!(reports.len(), 1);
    let report = &reports[0];
    assert_eq!(report.key.check, check("rust:a::flaky_input"));
    assert_eq!(
        (report.cached, report.rerun),
        (Outcome::Pass, Outcome::Fail)
    );
    assert_eq!(report.cached_record, RecordId(77));
    assert_eq!(report.cached_machine, MachineId::new("m-2").unwrap());
    assert_eq!(report.rerun_machine, MachineId::new("m-1").unwrap());
    assert_eq!(report.rerun_record, RecordId(91), "the rerun's own record");
    assert_eq!(report.commit, commit("bbbbbbb"));
    assert_eq!(report.output, "assertion failed");
    assert_eq!(f.host.leased.lock().unwrap()[0], commit("bbbbbbb"));

    // An audited hit is not rerun.
    let reply = verb.run(&f.ctx, &request("audit", &["--now"]));
    assert!(reply.text.contains("no cache hit"), "{}", reply.text);
    assert_eq!(f.reruns.runs.lock().unwrap().len(), 1);
}

#[test]
fn an_audit_that_agrees_reports_nothing() {
    let f = Fixture::new();
    let idle = Arc::new(f.idle());
    idle.hit(&cached("rust:a::b", Outcome::Pass), &commit("bbbbbbb"));
    let reply = AuditVerb { idle }.run(&f.ctx, &request("audit", &["--now"]));
    assert!(reply.text.contains("all agreed"), "{}", reply.text);
    assert!(f.hub.audit_reports().is_empty());
}

#[test]
fn about_one_hit_in_two_hundred_is_sampled_and_audited_in_idle_time() {
    let f = Fixture::new();
    let idle = f.idle();
    for n in 0..4000 {
        idle.hit(
            &cached(&format!("rust:a::t{n}"), Outcome::Pass),
            &commit("ccccccc"),
        );
    }
    let sampled = idle
        .state()
        .hits
        .values()
        .map(|day| day.sampled.len())
        .sum::<usize>();
    assert!((5..=40).contains(&sampled), "{sampled} sampled of 4000");
    // The same hits again the same day add nothing.
    let before = idle.state().clone();
    for n in 0..4000 {
        idle.hit(
            &cached(&format!("rust:a::t{n}"), Outcome::Pass),
            &commit("ccccccc"),
        );
    }
    assert_eq!(*idle.state(), before);
    // Idle time audits one hit per job.
    let mut jobs = 0;
    while idle.next_job(&f.ctx) == Some(IdleJob::Audit) {
        idle.run(&f.ctx, &IdleJob::Audit).unwrap();
        jobs += 1;
    }
    assert_eq!(jobs, sampled);
    assert_eq!(f.reruns.runs.lock().unwrap().len(), sampled);
}

#[test]
fn an_audit_stopped_midway_gives_the_same_report_when_started_again() {
    let f = Fixture::new();
    let idle = f.idle();
    idle.hit(&cached("rust:a::b", Outcome::Fail), &commit("bbbbbbb"));
    f.reruns.stop.store(true, Ordering::SeqCst);
    let result = catch_unwind(AssertUnwindSafe(|| idle.audit(&f.ctx, true)));
    assert!(result.is_err());
    f.reruns.stop.store(false, Ordering::SeqCst);
    let restarted = f.idle();
    assert_eq!(restarted.next_job(&f.ctx), Some(IdleJob::Audit));
    restarted.run(&f.ctx, &IdleJob::Audit).unwrap();
    let reports = f.hub.audit_reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(
        (reports[0].cached, reports[0].rerun),
        (Outcome::Fail, Outcome::Pass)
    );
    assert_eq!(
        restarted.next_job(&f.ctx).map(|j| j == IdleJob::Audit),
        Some(false)
    );
}

#[test]
fn a_rebuild_counts_the_compiler_units_and_leaves_the_files_as_they_were() {
    let f = Fixture::new();
    let idle = Arc::new(f.idle());
    let before = (f.tree_file("src/core.rs"), f.tree_file("src/other.rs"));
    let verb = RebuildVerb { idle: idle.clone() };
    let reply = verb.run(&f.ctx, &request("rebuild", &["core-body"]));
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert_eq!(
        reply.text,
        "core-body  45 billion  2 crates  at aaaaaaa\n  core                 33 billion\n  app                  12 billion\n"
    );
    assert_eq!(
        (f.tree_file("src/core.rs"), f.tree_file("src/other.rs")),
        before
    );
    let record = &f.hub.stored_records()[0];
    assert_eq!(record.verb, "rebuild");
    assert_eq!(record.family.as_deref(), Some("core-body"));
    assert_eq!(record.compile_units.len(), 3);
    // The tree's builds match the edit, which no commit holds.
    assert_eq!(f.host.released.lock().unwrap()[0].1, None);

    let stored = verb.run(&f.ctx, &request("rebuild", &["--last"]));
    assert_eq!(stored.text, reply.text);
    let edits = verb.run(&f.ctx, &request("rebuild", &["--list"]));
    assert!(edits.text.starts_with("core-body"), "{}", edits.text);
    let unknown = verb.run(&f.ctx, &request("rebuild", &["nothing"]));
    assert_eq!(unknown.exit, Exit::Usage);
}

#[test]
fn a_rebuild_stopped_midway_restores_the_file_and_measures_the_same_count() {
    let f = Fixture::new();
    let before = f.tree_file("src/core.rs");
    let job = IdleJob::Rebuild {
        name: "core-body".to_owned(),
    };
    f.compiler.stop.store(true, Ordering::SeqCst);
    stopped(&f.idle(), &f.ctx, &job);
    assert_ne!(
        f.tree_file("src/core.rs"),
        before,
        "the stop left the edit applied"
    );

    f.compiler.stop.store(false, Ordering::SeqCst);
    let restarted = f.idle();
    assert_eq!(restarted.next_job(&f.ctx), Some(job.clone()));
    restarted.run(&f.ctx, &job).unwrap();
    assert_eq!(f.tree_file("src/core.rs"), before);
    let counts = restarted.state().counts.clone();
    assert_eq!(counts.len(), 1);
    assert!((counts[0].instructions - 45e9).abs() < 1.0);
}

#[test]
fn a_rebuild_is_measured_again_after_a_hundred_commits() {
    let f = Fixture::new();
    let idle = f.idle();
    let job = IdleJob::Rebuild {
        name: "core-body".to_owned(),
    };
    assert_eq!(idle.next_job(&f.ctx), Some(job.clone()));
    idle.run(&f.ctx, &job).unwrap();
    assert_ne!(idle.next_job(&f.ctx), Some(job.clone()));
    let git = FakeGit::default();
    git.set_tip("master", commit("ddddddd"));
    let ctx = Context {
        git: Arc::new(git),
        ..f.ctx.clone()
    };
    f.host.between.store(99, Ordering::SeqCst);
    assert_ne!(idle.next_job(&ctx), Some(job.clone()));
    f.host.between.store(100, Ordering::SeqCst);
    assert_eq!(idle.next_job(&ctx), Some(job));
}

#[test]
fn a_machine_that_cannot_count_instructions_measures_no_rebuild() {
    let f = Fixture::new();
    let idle = Arc::new(f.idle_on(Os::Windows));
    assert_eq!(
        idle.next_job(&f.ctx)
            .map(|j| matches!(j, IdleJob::Rebuild { .. })),
        Some(false)
    );
    let reply = RebuildVerb { idle: idle.clone() }.run(&f.ctx, &request("rebuild", &["core-body"]));
    assert_eq!(reply.exit, Exit::Usage, "{}", reply.text);
    assert!(
        reply.text.contains("cannot count instructions"),
        "{}",
        reply.text
    );
    assert!(f.hub.stored_records().is_empty());
    assert!(idle.state().counts.is_empty());
}

#[test]
fn units_without_instruction_counts_make_no_rebuild_count() {
    let unit = |instructions| CompileUnit {
        crate_name: "core".to_owned(),
        mode: "build".to_owned(),
        instructions,
        cpu_s: 1.0,
        rebuilt: true,
    };
    let at = Timestamp::EPOCH;
    let counted = [unit(Some(5.0))];
    assert!(RebuildCount::from_units("e", commit("aaaaaaa"), at, &counted).is_some());
    let mixed = [unit(Some(5.0)), unit(None)];
    assert!(RebuildCount::from_units("e", commit("aaaaaaa"), at, &mixed).is_none());
    assert!(RebuildCount::from_units("e", commit("aaaaaaa"), at, &[]).is_none());
}

#[test]
fn busy_landing_trees_leave_the_work_for_later() {
    let f = Fixture::new();
    f.host.busy.store(true, Ordering::SeqCst);
    let idle = Arc::new(f.idle());
    let reply = RebuildVerb { idle: idle.clone() }.run(&f.ctx, &request("rebuild", &["core-body"]));
    assert_eq!(reply.exit, Exit::Conflict);
    idle.hit(&cached("rust:a::b", Outcome::Pass), &commit("bbbbbbb"));
    let summary = idle.audit(&f.ctx, true).unwrap();
    assert!(summary.busy);
    assert!(
        summary.render().contains("no spare landing tree was free"),
        "{}",
        summary.render()
    );
    assert!(idle.audits_due());
    assert!(Path::new(&f.dir).exists());
}
