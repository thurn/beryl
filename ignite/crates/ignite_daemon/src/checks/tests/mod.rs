//! The check runner against the fakes: a launcher that answers from a script, a fake hub, fake
//! builds and git, and a tree in a temporary directory.

mod budget;
mod compile;
mod costs;
mod express;
mod fail_fast;
mod hardening;
mod listings;
mod observed;
mod plan;
mod scope;
mod verdicts;

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ignite_core::config::{Config, Os};
use ignite_core::fakes::{FakeBuilds, FakeClock, FakeGit, FakeHub};
use ignite_core::ids::{CheckName, Commit, MachineId, Oid, TreeId};
use ignite_core::inputs::CrateGraph;
use ignite_core::manifest::Manifest;
use ignite_core::protocol::client::{Exit, Request};
use ignite_core::protocol::hub::{
    AreaStats, CheckState, CheckStats, CheckStatsReply, NewResult, ResultKey,
};
use ignite_core::protocol::{Class, Outcome, Usage};
use ignite_core::time::Timestamp;
use ignite_core::traits::{CommandSpec, Hub, Launcher, Result, RunResult};

use super::*;
use crate::cache::ResultCache;
use crate::server::{Context, Verb};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

/// A fresh directory under the system's temporary directory.
fn temp_dir() -> PathBuf {
    let n = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("ignite-checks-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Answers `lister` with the listing, `runner <checks>` by writing each check's next scripted
/// outcome (a pass when nothing is scripted, nothing for `None`) to `IGNITE_RESULTS`, and
/// cargo-nextest's commands with their scripted output. `cannot-run` writes an incomplete line
/// with that reason; `hang` writes nothing and makes the run end as hung.
#[derive(Default)]
struct ScriptLauncher {
    dir: PathBuf,
    listing: Mutex<String>,
    list_exit: Mutex<i32>,
    outcomes: Mutex<HashMap<String, VecDeque<Option<&'static str>>>>,
    nextest_run: Mutex<String>,
    /// Outputs that the next cargo-nextest runs fail with, exit code 101, one each, as when
    /// their Cargo build fails.
    nextest_failures: Mutex<VecDeque<String>>,
    runs: Mutex<Vec<CommandSpec>>,
    files: AtomicU64,
    /// Called when a family's `run` command starts, to change the tree under it.
    on_run: Mutex<Option<Box<dyn Fn() + Send>>>,
}

impl ScriptLauncher {
    fn script(&self, check: &str, outcomes: &[Option<&'static str>]) {
        let mut scripted = self.outcomes.lock().unwrap();
        scripted.insert(check.to_owned(), outcomes.iter().copied().collect());
    }

    fn commands(&self, program: &str) -> Vec<Vec<String>> {
        let runs = self.runs.lock().unwrap();
        runs.iter()
            .filter(|s| s.args[0] == program)
            .map(|s| s.args.clone())
            .collect()
    }
}

impl Launcher for ScriptLauncher {
    fn run(&self, spec: &CommandSpec) -> RunResult {
        self.runs.lock().unwrap().push(spec.clone());
        let mut exit = Some(0);
        let mut hung = false;
        let text = match spec.args[0].as_str() {
            "lister" => {
                exit = Some(*self.list_exit.lock().unwrap());
                self.listing.lock().unwrap().clone()
            }
            // A `list` command that always fails.
            "broken" => {
                exit = Some(1);
                String::new()
            }
            "runner" => {
                if let Some(hook) = self.on_run.lock().unwrap().as_ref() {
                    hook();
                }
                let file = spec
                    .env
                    .iter()
                    .find(|(k, _)| k == adapters::RESULTS_ENV)
                    .map(|(_, v)| PathBuf::from(v))
                    .unwrap();
                let mut scripted = self.outcomes.lock().unwrap();
                let mut lines = String::new();
                for check in &spec.args[1..] {
                    let next = scripted
                        .get_mut(check)
                        .and_then(VecDeque::pop_front)
                        .unwrap_or(Some("pass"));
                    if next == Some("hang") {
                        hung = true;
                    } else if next == Some("cannot-run") {
                        let _ = writeln!(
                            lines,
                            "{{\"check\": \"{check}\", \"outcome\": \"incomplete\", \"reason\": \"cannot-run\", \"message\": \"no simulator\"}}"
                        );
                    } else if let Some(outcome) = next {
                        let _ = writeln!(
                            lines,
                            "{{\"check\": \"{check}\", \"outcome\": \"{outcome}\", \"at\": \"src/a.py:3\"}}"
                        );
                    }
                }
                fs::write(file, lines).unwrap();
                String::new()
            }
            "cargo" if spec.args[2] == "list" => self.listing.lock().unwrap().clone(),
            "cargo" => {
                // A real run's test runner appends one unit per test process to the file.
                if let Some((_, file)) = spec.env.iter().find(|(k, _)| k == crate::wrap::UNITS_ENV)
                {
                    let unit = "{\"crate\":\"core\",\"mode\":\"run\",\"instructions\":5.0e8,\"cpu_s\":0.25,\"rebuilt\":false}\n";
                    fs::write(file, unit).unwrap();
                }
                if let Some(failure) = self.nextest_failures.lock().unwrap().pop_front() {
                    exit = Some(101);
                    failure
                } else {
                    self.nextest_run.lock().unwrap().clone()
                }
            }
            _ => String::new(),
        };
        let n = self.files.fetch_add(1, Ordering::Relaxed);
        let output = self.dir.join(format!("output-{n}.txt"));
        fs::write(&output, text).unwrap();
        if hung {
            exit = None;
        }
        RunResult {
            exit,
            output,
            usage: Usage {
                wall_s: 1.0,
                cpu_s: 2.0,
                instructions: Some(6_000_000_000),
                ..Usage::default()
            },
            hung,
        }
    }
}

struct FakeTrees {
    dir: PathBuf,
    changed: Vec<String>,
    /// The tracked files; the default three when empty.
    tracked: Vec<String>,
    graph: CrateGraph,
}

impl FakeTrees {
    fn new(dir: &Path, changed: &[&str]) -> FakeTrees {
        FakeTrees {
            dir: dir.to_path_buf(),
            changed: changed.iter().map(|s| (*s).to_owned()).collect(),
            tracked: Vec::new(),
            graph: CrateGraph::default(),
        }
    }
}

impl TreeSource for FakeTrees {
    fn tree_of(&self, cwd: &Path) -> Option<TreeId> {
        cwd.starts_with(&self.dir)
            .then(|| TreeId::new("tree-01").unwrap())
    }

    fn path(&self, _tree: &TreeId) -> Option<PathBuf> {
        Some(self.dir.clone())
    }

    fn tracked(&self, _path: &Path) -> Result<Vec<String>> {
        if !self.tracked.is_empty() {
            return Ok(self.tracked.clone());
        }
        Ok(vec![
            "src/a.py".to_owned(),
            "data/core.json".to_owned(),
            "game_rules/tests/entering.rs".to_owned(),
        ])
    }

    fn crate_graph(&self, _path: &Path, _tracked: &[String]) -> Result<CrateGraph> {
        Ok(self.graph.clone())
    }

    fn head(&self, _path: &Path) -> Result<Commit> {
        Ok(Commit::new("abc1234").unwrap())
    }

    fn changed(&self, _path: &Path, _base: &str) -> Result<Vec<String>> {
        // A first path of `<error>` stands for a tree git cannot compare with the base.
        if self.changed.first().is_some_and(|p| p == "<error>") {
            return Err(ignite_core::traits::Error::Io("git failed".to_owned()));
        }
        Ok(self.changed.clone())
    }
}

#[derive(Default)]
struct FakeHistory {
    replies: BTreeMap<String, CheckStatsReply>,
}

impl CheckHistory for FakeHistory {
    fn stats(&self, family: &str) -> Result<CheckStatsReply> {
        Ok(self
            .replies
            .get(family)
            .cloned()
            .unwrap_or(CheckStatsReply {
                startup_billions: None,
                startup_cpu_s: None,
                family: family.to_owned(),
                family_rate: 0.0,
                checks: Vec::new(),
            }))
    }
}

const MANIFEST: &str = r#"
[product.deps]
build = "make deps"
inputs = ["src"]
outputs = ["out"]

[family.unit]
kind = "regression"
adapter = "jsonl"
platforms = "any"
needs = ["deps"]
list = "lister"
run = "runner {checks}"
inputs = ["src"]

[family.rs]
kind = "regression"
adapter = "nextest"
platforms = ["macos"]
needs = ["deps"]
inputs = ["src"]
"#;

struct World {
    ctx: Context,
    checks: Arc<Checks>,
    hub: Arc<FakeHub>,
    builds: Arc<FakeBuilds>,
    launcher: Arc<ScriptLauncher>,
    git: Arc<FakeGit>,
    dir: PathBuf,
}

fn world(manifest: &str, changed: &[&str], history: FakeHistory) -> World {
    let dir = temp_dir();
    let config = Config::parse(
        "ignite.toml",
        r#"
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
        threshold = 0.004
        seconds = 60
        [machine.local]
        ticket_trees = 1
        landing_trees = 1
        "#,
    )
    .unwrap();
    let hub = Arc::new(FakeHub::default());
    let builds = Arc::new(FakeBuilds::default());
    let git = Arc::new(FakeGit::default());
    git.set_object("src", Oid::new("a".repeat(40)).unwrap());
    git.set_tip("staging", Commit::new("ba5e123").unwrap());
    let launcher = Arc::new(ScriptLauncher {
        dir: dir.clone(),
        ..ScriptLauncher::default()
    });
    let clock = Arc::new(FakeClock::new(Timestamp::EPOCH));
    let machine = MachineId::new("m-1").unwrap();
    let ctx = Context {
        config,
        repo: dir.clone(),
        machine: machine.clone(),
        clock: clock.clone(),
        hub: hub.clone(),
        git: git.clone(),
        launcher: launcher.clone(),
        builds: builds.clone(),
        session: None,
        tree: None,
    };
    let manifest =
        Manifest::from_files(&[(PathBuf::from("checks/test.toml"), manifest.to_owned())]).unwrap();
    let settings = CheckSettings {
        manifest,
        os: Os::Macos,
        families: None,
        toolchain: "1.0".to_owned(),
        outputs: dir.join("results"),
        cores: 4,
        ignite: "test".to_owned(),
        ignite_exe: PathBuf::from("/bin/ignite"),
        shims: dir.join("shims"),
    };
    let cache = ResultCache::new(
        ignite_store::local::open_in_memory().unwrap(),
        hub.clone(),
        machine,
        clock,
    );
    let trees = FakeTrees::new(&dir, changed);
    let checks = Arc::new(Checks::new(
        settings,
        cache,
        Box::new(trees),
        Box::new(history),
    ));
    World {
        ctx,
        checks,
        hub,
        builds,
        launcher,
        git,
        dir,
    }
}

/// Another daemon on this world's machine, hub, builds and launcher: with its own memory, the
/// local database `local`, and a tree that `trees` describes. A daemon restarted on the same
/// machine reopens the same local database.
fn daemon(world: &World, local: rusqlite::Connection, trees: FakeTrees) -> Arc<Checks> {
    let cache = ResultCache::new(
        local,
        world.hub.clone(),
        world.ctx.machine.clone(),
        world.ctx.clock.clone(),
    );
    Arc::new(Checks::new(
        world.checks.settings.clone(),
        cache,
        Box::new(trees),
        Box::new(FakeHistory::default()),
    ))
}

fn jsonl_world() -> World {
    let world = world(MANIFEST, &[], FakeHistory::default());
    // The listing also holds a cargo-nextest listing, for the manifest's Rust family, which
    // must list at least one test.
    *world.launcher.listing.lock().unwrap() =
        format!("{{\"check\": \"unit:a\"}}\n{{\"check\": \"unit:b\"}}\n{NEXTEST_LISTING}\n");
    world
}

fn name(text: &str) -> CheckName {
    CheckName::new(text).unwrap()
}

fn options(rerun: bool) -> RunOptions {
    RunOptions {
        verb: "test".to_owned(),
        class: Class::Agent,
        rerun,
        ticket: None,
        session: None,
    }
}

fn run(world: &World, names: &[&str]) -> CheckRun {
    let names: Vec<CheckName> = names.iter().map(|n| name(n)).collect();
    let tree = TreeId::new("tree-01").unwrap();
    world.checks.run(&world.ctx, &tree, &names, &options(false))
}

fn outcome(run: &CheckRun, check: &str) -> Outcome {
    run.outcomes
        .iter()
        .find(|(c, _)| c.as_str() == check)
        .map(|(_, o)| *o)
        .unwrap()
}

/// The result key of `check` in this world's tree.
fn key_of(world: &World, check: &str) -> ResultKey {
    let mut probe = CheckRun::default();
    let tree = world.checks.open(&TreeId::new("tree-01").unwrap()).unwrap();
    let family = name(check).family().to_owned();
    let prepared = world
        .checks
        .prepare(&world.ctx, &tree, &[&family], &options(false), &mut probe);
    let keyed = prepared[0]
        .keyed
        .iter()
        .find(|k| k.listed.check.as_str() == check)
        .unwrap();
    keyed.key.clone()
}

/// What the hub and the local copy hold for `check`'s key in this world.
fn cached(world: &World, check: &str) -> Option<Outcome> {
    world.checks.cache().lookup(&[key_of(world, check)])[0]
        .as_ref()
        .map(|r| r.outcome)
}

#[test]
fn a_family_reads_the_tree_once_to_hash_all_its_checks() {
    // Reading the tree runs git; one reading per check made a round of thousands of checks
    // take most of an hour.
    let world = jsonl_world();
    let tree = world.checks.open(&TreeId::new("tree-01").unwrap()).unwrap();
    let before = world.git.object_id_calls();
    let mut probe = CheckRun::default();
    let prepared = world
        .checks
        .prepare(&world.ctx, &tree, &["unit"], &options(false), &mut probe);
    assert!(prepared[0].keyed.len() >= 2, "{}", prepared[0].keyed.len());
    // One reading for the family's listing hash, one for every check's key.
    assert_eq!(world.git.object_id_calls() - before, 2);
}

/// A tip round runs a family in batches of a few dozen checks. Listing and keying the whole
/// family again for each batch spent most of a round between commands; the batches now run
/// from one listing, and a batch the claim refuses is skipped.
#[test]
fn batches_of_one_family_run_from_one_listing() {
    let world = jsonl_world();
    let tree = TreeId::new("tree-01").unwrap();
    let batches = vec![
        (0, vec![name("unit:a")]),
        (1, vec![name("unit:b")]),
        (2, vec![name("unit:a")]),
    ];
    let claim = |batch: u32| batch != 2;
    let (run, ran) =
        world
            .checks
            .run_batches(&world.ctx, &tree, "unit", &batches, &options(false), &claim);
    assert_eq!(ran, 2);
    assert!(run.passed(), "{:?}", run.failures);
    assert_eq!(run.outcomes.len(), 2);
    assert_eq!(world.launcher.commands("lister").len(), 1);
    assert_eq!(world.launcher.commands("runner").len(), 2);
}

#[test]
fn a_cached_pass_runs_nothing_and_builds_nothing() {
    let world = jsonl_world();
    let first = run(&world, &["unit:a", "unit:b"]);
    assert!(first.passed());
    assert_eq!((first.ran, first.from_cache), (2, 0));
    assert_eq!(world.launcher.commands("runner").len(), 1);
    let second = run(&world, &["unit:a", "unit:b"]);
    assert!(second.passed());
    assert_eq!((second.ran, second.from_cache), (0, 2));
    assert_eq!(world.launcher.commands("runner").len(), 1, "nothing ran");
    // `list` asks for the products it needs, which are current, so nothing builds.
    assert!(second.builds.is_empty(), "nothing was built");
    let records = world.hub.stored_records();
    let last = records.last().unwrap();
    assert_eq!((last.verb.as_str(), last.wall_s), ("cached", 0.0));
    assert_eq!(last.family.as_deref(), Some("unit"));
}

#[test]
fn a_changed_digest_misses_the_cache_and_list_is_told_the_base() {
    let world = jsonl_world();
    let listing = |digest: &str| {
        *world.launcher.listing.lock().unwrap() =
            format!("{{\"check\": \"unit:a\", \"digest\": \"{digest}\"}}\n");
    };
    listing("d1");
    assert_eq!(run(&world, &["unit:a"]).ran, 1);
    assert_eq!(
        run(&world, &["unit:a"]).from_cache,
        1,
        "the same digest is a hit"
    );
    listing("d2");
    assert_eq!(run(&world, &["unit:a"]).ran, 1, "a new digest runs again");
    let runs = world.launcher.runs.lock().unwrap();
    let list = runs.iter().find(|s| s.args[0] == "lister").unwrap();
    let base = ("IGNITE_BASE".to_owned(), "ba5e123".to_owned());
    assert!(
        list.env.contains(&base),
        "list sees the base its digest covers"
    );
}

#[test]
fn a_failure_then_a_pass_is_flaky_and_passes() {
    let world = jsonl_world();
    world
        .launcher
        .script("unit:b", &[Some("fail"), Some("pass")]);
    let run = run(&world, &["unit:a", "unit:b"]);
    assert!(run.passed());
    assert_eq!(outcome(&run, "unit:a"), Outcome::Pass);
    assert_eq!(outcome(&run, "unit:b"), Outcome::Flaky);
    let runner = world.launcher.commands("runner");
    assert_eq!(runner.len(), 2);
    assert_eq!(runner[1][1..], ["unit:b"], "only the failure runs again");
    let flaky = world.hub.flaky_reports();
    assert_eq!(flaky.len(), 1);
    assert_eq!(flaky[0].check.as_str(), "unit:b");
    assert!(
        !flaky[0].command.is_empty() && !flaky[0].always,
        "the report names the command the check failed in"
    );
    assert_eq!(
        cached(&world, "unit:b"),
        None,
        "a flaky result is not cached"
    );
}

#[test]
fn a_failure_twice_is_a_failure_and_is_cached() {
    let world = jsonl_world();
    world
        .launcher
        .script("unit:b", &[Some("fail"), Some("fail")]);
    let run = run(&world, &["unit:a", "unit:b"]);
    assert!(!run.passed());
    assert_eq!(outcome(&run, "unit:b"), Outcome::Fail);
    assert_eq!(run.failures[0].at.as_deref(), Some("src/a.py:3"));
    assert_eq!(cached(&world, "unit:b"), Some(Outcome::Fail));
}

#[test]
fn a_missing_report_is_incomplete_after_three_tries_and_never_cached() {
    let world = jsonl_world();
    world.launcher.script("unit:b", &[None, None, None]);
    let run = run(&world, &["unit:a", "unit:b"]);
    assert!(!run.passed());
    assert_eq!(outcome(&run, "unit:a"), Outcome::Pass);
    assert_eq!(outcome(&run, "unit:b"), Outcome::Incomplete);
    assert_eq!(world.launcher.commands("runner").len(), 3);
    assert_eq!(cached(&world, "unit:b"), None);
    assert_eq!(cached(&world, "unit:a"), Some(Outcome::Pass));
}

#[test]
fn a_failing_list_leaves_the_family_incomplete() {
    let world = jsonl_world();
    *world.launcher.list_exit.lock().unwrap() = 1;
    let run = run(&world, &["unit:a"]);
    assert!(!run.passed());
    assert_eq!(run.incomplete_families.len(), 1);
    assert_eq!(world.launcher.commands("lister").len(), 3);
    assert!(world.launcher.commands("runner").is_empty());
}

#[test]
fn with_the_hub_down_checks_run_from_the_local_copy() {
    let world = jsonl_world();
    run(&world, &["unit:a"]);
    world.hub.set_unreachable(true);
    let again = run(&world, &["unit:a", "unit:b"]);
    assert_eq!(outcome(&again, "unit:a"), Outcome::Pass);
    assert_eq!(outcome(&again, "unit:b"), Outcome::Pass);
    assert_eq!((again.from_cache, again.ran), (1, 1));
    let kinds: Vec<String> = world
        .checks
        .cache()
        .queued()
        .into_iter()
        .map(|(kind, _)| kind)
        .collect();
    assert!(kinds.contains(&"result".to_owned()));
    assert!(kinds.contains(&"record".to_owned()));
}

const NEXTEST_LISTING: &str = r#"{"rust-suites": {"core": {"binary-id": "core", "binary-name": "core", "package-name": "core", "testcases": {"a": {"ignored": false, "filter-match": {"status": "matches"}}}}}}"#;

#[test]
fn a_cached_nextest_family_neither_builds_nor_lists_again() {
    let world = world(MANIFEST, &[], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() = NEXTEST_LISTING.to_owned();
    *world.launcher.nextest_run.lock().unwrap() =
        "{\"type\":\"test\",\"event\":\"ok\",\"name\":\"core::core$a\"}\n".to_owned();
    let first = run(&world, &["rs:core::a"]);
    assert!(first.passed(), "{first:?}");
    let launched = world.launcher.runs.lock().unwrap().len();
    let builds = world.builds.requests().len();
    assert_eq!(
        world.builds.requests()[1].2,
        ["core"],
        "the run builds its package"
    );

    let second = run(&world, &["rs:core::a"]);
    assert_eq!(second.from_cache, 1);
    assert_eq!(world.launcher.runs.lock().unwrap().len(), launched);
    assert_eq!(world.builds.requests().len(), builds);
}

#[test]
fn package_inputs_enter_only_their_own_packages_keys() {
    // The Rust family is the manifest's last table, so this extends it.
    let manifest = format!("{MANIFEST}\n[family.rs.package_inputs]\ncore = [\"data/core.json\"]\n");
    let world = world(&manifest, &[], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() = NEXTEST_LISTING.replace(
        "}}}}}}",
        "}}}}, \"other\": {\"binary-id\": \"other\", \"binary-name\": \"other\", \
         \"package-name\": \"other\", \"testcases\": {\"b\": {\"ignored\": false, \
         \"filter-match\": {\"status\": \"matches\"}}}}}}",
    );
    world
        .git
        .set_object("data/core.json", Oid::new("c".repeat(40)).unwrap());
    let core = key_of(&world, "rs:core::a");
    let other = key_of(&world, "rs:other::b");

    world
        .git
        .set_object("data/core.json", Oid::new("d".repeat(40)).unwrap());
    assert_ne!(key_of(&world, "rs:core::a"), core);
    assert_eq!(key_of(&world, "rs:other::b"), other);
}

#[test]
fn test_resolves_names_by_substring_and_pattern() {
    let world = jsonl_world();
    let request = |args: &[&str]| Request {
        verb: "test".to_owned(),
        args: args.iter().map(|s| (*s).to_owned()).collect(),
        cwd: world.dir.join("sub"),
        session: None,
        wait_s: 1,
        pid: None,
    };
    let verb = TestVerb {
        checks: world.checks.clone(),
        express: None,
    };
    let reply = verb.run(&world.ctx, &request(&["it:b"]));
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert_eq!(world.launcher.commands("runner")[0][1..], ["unit:b"]);
    let reply = verb.run(&world.ctx, &request(&["u*:?"]));
    assert_eq!(
        reply.text,
        "listed   nothing listed afresh, 1 kept\nran      2 checks: all passed (1 from cache)\n"
    );
    // Every family's listing is kept by now, so a name that matches none is refused with a
    // hint and nothing is listed or built again.
    let listers = world.launcher.commands("lister").len();
    let runners = world.launcher.commands("runner").len();
    let reply = verb.run(&world.ctx, &request(&["nothing", "built_line"]));
    assert_eq!(reply.exit, Exit::Usage);
    assert!(
        reply
            .text
            .starts_with("no check matches nothing, built_line\nchecks are named <family>:<check>"),
        "{}",
        reply.text
    );
    assert_eq!(world.launcher.commands("lister").len(), listers);
    assert_eq!(world.launcher.commands("runner").len(), runners);
}

#[test]
fn test_finds_a_bare_name_in_a_familys_newest_listing_after_its_inputs_change() {
    let world = jsonl_world();
    let request = |args: &[&str]| Request {
        verb: "test".to_owned(),
        args: args.iter().map(|s| (*s).to_owned()).collect(),
        cwd: world.dir.join("sub"),
        session: None,
        wait_s: 1,
        pid: None,
    };
    let verb = TestVerb {
        checks: world.checks.clone(),
        express: None,
    };
    let reply = verb.run(&world.ctx, &request(&["it:b"]));
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    // A change to a file every family reads, as a change to one card is in a game's checkout,
    // leaves no family with a kept listing for the tree's inputs.
    world
        .git
        .set_object("src", Oid::new("e".repeat(40)).unwrap());
    let listers = world.launcher.commands("lister").len();
    let cargo = world.launcher.commands("cargo").len();
    let reply = verb.run(&world.ctx, &request(&["it:b"]));
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert_eq!(
        world.launcher.commands("lister").len(),
        listers + 1,
        "the family whose newest listing holds the check is listed again"
    );
    assert_eq!(
        world.launcher.commands("cargo").len(),
        cargo,
        "the Rust family, whose newest listing holds no such check, is neither listed nor run"
    );
    // A name no listing holds may name a check added since, so every family without a kept
    // listing for the tree's inputs is listed to find out.
    let reply = verb.run(&world.ctx, &request(&["zzz"]));
    assert_eq!(reply.exit, Exit::Usage, "{}", reply.text);
    assert!(world.launcher.commands("cargo").len() > cargo);
}
