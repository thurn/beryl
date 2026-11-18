//! The check runner against the fakes: a launcher that answers from a script, a fake hub, fake
//! builds and git, and a tree in a temporary directory.

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
/// cargo-nextest's commands with their scripted output.
#[derive(Default)]
struct ScriptLauncher {
    dir: PathBuf,
    listing: Mutex<String>,
    list_exit: Mutex<i32>,
    outcomes: Mutex<HashMap<String, VecDeque<Option<&'static str>>>>,
    nextest_run: Mutex<String>,
    runs: Mutex<Vec<CommandSpec>>,
    files: AtomicU64,
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
        let mut exit = 0;
        let text = match spec.args[0].as_str() {
            "lister" => {
                exit = *self.list_exit.lock().unwrap();
                self.listing.lock().unwrap().clone()
            }
            "runner" => {
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
                    if let Some(outcome) = next {
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
            "cargo" => self.nextest_run.lock().unwrap().clone(),
            _ => String::new(),
        };
        let n = self.files.fetch_add(1, Ordering::Relaxed);
        let output = self.dir.join(format!("output-{n}.txt"));
        fs::write(&output, text).unwrap();
        RunResult {
            exit: Some(exit),
            output,
            usage: Usage {
                wall_s: 1.0,
                ..Usage::default()
            },
        }
    }
}

struct FakeTrees {
    dir: PathBuf,
    changed: Vec<String>,
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
        Ok(vec!["src/a.py".to_owned()])
    }

    fn crate_graph(&self, _path: &Path, _tracked: &[String]) -> Result<CrateGraph> {
        Ok(CrateGraph::default())
    }

    fn head(&self, _path: &Path) -> Result<Commit> {
        Ok(Commit::new("abc1234").unwrap())
    }

    fn changed(&self, _path: &Path, _base: &str) -> Result<Vec<String>> {
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
        cap = 400
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
        git,
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
        cores: 1,
        ignite: "test".to_owned(),
    };
    let cache = ResultCache::new(
        ignite_store::local::open_in_memory().unwrap(),
        hub.clone(),
        machine,
        clock,
    );
    let trees = FakeTrees {
        dir: dir.clone(),
        changed: changed.iter().map(|s| (*s).to_owned()).collect(),
    };
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
        dir,
    }
}

fn jsonl_world() -> World {
    let world = world(MANIFEST, &[], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() =
        "{\"check\": \"unit:a\"}\n{\"check\": \"unit:b\"}\n".to_owned();
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

/// What the hub and the local copy hold for `check`'s key in this world.
fn cached(world: &World, check: &str) -> Option<Outcome> {
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
    world
        .checks
        .cache()
        .lookup(std::slice::from_ref(&keyed.key))[0]
        .as_ref()
        .map(|r| r.outcome)
}

#[test]
fn a_cached_pass_runs_nothing_and_builds_nothing() {
    let world = jsonl_world();
    let first = run(&world, &["unit:a", "unit:b"]);
    assert!(first.passed());
    assert_eq!((first.ran, first.from_cache), (2, 0));
    assert_eq!(world.launcher.commands("runner").len(), 1);
    let builds = world.builds.requests().len();

    let second = run(&world, &["unit:a", "unit:b"]);
    assert!(second.passed());
    assert_eq!((second.ran, second.from_cache), (0, 2));
    assert_eq!(world.launcher.commands("runner").len(), 1, "nothing ran");
    assert_eq!(world.builds.requests().len(), builds, "nothing was built");
    let records = world.hub.stored_records();
    let last = records.last().unwrap();
    assert_eq!((last.verb.as_str(), last.wall_s), ("cached", 0.0));
    assert_eq!(last.family.as_deref(), Some("unit"));
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
    };
    let reply = verb.run(&world.ctx, &request(&["it:b"]));
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert_eq!(world.launcher.commands("runner")[0][1..], ["unit:b"]);
    let reply = verb.run(&world.ctx, &request(&["u*:?"]));
    assert_eq!(reply.text, "ran      2 checks: all passed (1 from cache)\n");
    let reply = verb.run(&world.ctx, &request(&["nothing"]));
    assert_eq!(
        (reply.exit, reply.text.as_str()),
        (Exit::Usage, "no check matches nothing")
    );
}

/// A history in which the gate's plan reads as the example under "The threshold and the cap"
/// in `docs/ignite.md`, with neutral names.
fn seeded() -> World {
    let area = "game_rules/tests/";
    let stats = |check: &str, runs: u32, failures: u32, cost: f64| CheckStats {
        check: name(check),
        family: name(check).family().to_owned(),
        state: CheckState::On,
        areas: vec![AreaStats {
            area: area.to_owned(),
            runs,
            failures,
        }],
        median_cost: Some(cost),
        unit: "instructions".to_owned(),
    };
    let mut script = vec![stats("script:aura/enters-attached/you", 74, 9, 0.79)];
    let mut listing = vec!["script:aura/enters-attached/you".to_owned()];
    for (prefix, count, runs, failures, cost) in [
        ("run", 14, 100, 5, 6.65),
        ("cached", 44, 100, 5, 1.0),
        ("tip", 1203, 1000, 0, 1.0),
    ] {
        for i in 0..count {
            let check = format!("script:s/{prefix}-{i}");
            script.push(stats(&check, runs, failures, cost));
            listing.push(check);
        }
    }
    listing.extend(
        [
            "rust:game_rules/tests/entering.rs::a",
            "rust:game_rules/tests/entering.rs::b",
            "rust:game_rules/other.rs::c",
        ]
        .map(str::to_owned),
    );
    for lint in ["clippy", "fmt", "card-names", "file-length"] {
        listing.push(format!("lint:{lint}"));
    }
    let reply = |family: &str, checks: Vec<CheckStats>| CheckStatsReply {
        family: family.to_owned(),
        family_rate: 0.02,
        checks,
    };
    let mut history = FakeHistory::default();
    history
        .replies
        .insert("script".to_owned(), reply("script", script));
    history.replies.insert(
        "rust".to_owned(),
        reply(
            "rust",
            vec![stats("rust:game_rules/other.rs::c", 1000, 0, 1.1)],
        ),
    );
    let family = |name: &str, always: bool| {
        format!(
            "[family.{name}]\nkind = \"regression\"\nadapter = \"jsonl\"\nplatforms = \"any\"\n\
             always = {always}\nlist = \"lister\"\nrun = \"runner {{checks}}\"\ninputs = [\"src\"]\n"
        )
    };
    let manifest = [
        family("script", false),
        family("rust", false),
        family("lint", true),
    ]
    .join("\n");
    let world = world(&manifest, &["game_rules/tests/entering.rs"], history);
    let lines: Vec<String> = listing
        .iter()
        .map(|c| format!("{{\"check\": \"{c}\"}}"))
        .collect();
    *world.launcher.listing.lock().unwrap() = lines.join("\n");
    store_cached(&world);
    world
}

/// Stores passes for the always set and the 44 cached scored checks of [`seeded`].
fn store_cached(world: &World) {
    let mut probe = CheckRun::default();
    let tree = world.checks.open(&TreeId::new("tree-01").unwrap()).unwrap();
    let prepared = world.checks.prepare(
        &world.ctx,
        &tree,
        &["script", "lint"],
        &options(false),
        &mut probe,
    );
    let keys: Vec<ResultKey> = prepared
        .iter()
        .flat_map(|p| &p.keyed)
        .filter(|k| {
            let check = k.listed.check.as_str();
            check.starts_with("lint:") || check.starts_with("script:s/cached-")
        })
        .map(|k| k.key.clone())
        .collect();
    assert_eq!(keys.len(), 48);
    let results: Vec<NewResult> = keys
        .into_iter()
        .map(|key| NewResult {
            key,
            outcome: Outcome::Pass,
            machine: MachineId::new("m-2").unwrap(),
            record: ignite_core::ids::RecordId(1),
            output: None,
        })
        .collect();
    world.hub.store(&results).unwrap();
}

#[test]
fn check_plan_prints_the_documented_example() {
    let world = seeded();
    let verb = CheckVerb {
        checks: world.checks.clone(),
    };
    let reply = verb.run(
        &world.ctx,
        &Request {
            verb: "check".to_owned(),
            args: vec!["--plan".to_owned()],
            cwd: world.dir.clone(),
            session: None,
            wait_s: 1,
            pid: None,
        },
    );
    assert_eq!(reply.exit, Exit::Success);
    assert_eq!(
        reply.text,
        "\
always   4 checks (card-names, clippy, file-length, fmt)
gate     61 checks above 0.004; 44 cached, 17 to run, about 96 billion
tip      1,204 affected checks below the threshold
top      rust:game_rules/tests/entering.rs::*      0.91  edited by this change
         script:aura/enters-attached/you           0.13  9 of 74 runs failed
"
    );
    assert!(
        world.launcher.commands("runner").is_empty(),
        "the plan runs nothing"
    );
}

#[test]
fn check_runs_the_gate_and_answers_the_rest_from_the_cache() {
    let world = seeded();
    let verb = CheckVerb {
        checks: world.checks.clone(),
    };
    let reply = verb.run(
        &world.ctx,
        &Request {
            verb: "check".to_owned(),
            args: Vec::new(),
            cwd: world.dir.clone(),
            session: None,
            wait_s: 1,
            pid: None,
        },
    );
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(
        reply
            .text
            .starts_with("gate     65 checks: 48 from cache, 17 run, all passed in "),
        "{}",
        reply.text
    );
    assert!(
        reply
            .text
            .ends_with("tip      1,204 checks left to tip verification\n")
    );
}

#[test]
fn why_prints_the_record_and_the_inputs() {
    let world = seeded();
    let verb = WhyVerb {
        checks: world.checks.clone(),
    };
    let request = |args: &[&str]| Request {
        verb: "why".to_owned(),
        args: args.iter().map(|s| (*s).to_owned()).collect(),
        cwd: world.dir.clone(),
        session: None,
        wait_s: 1,
        pid: None,
    };
    let reply = verb.run(&world.ctx, &request(&["script:aura/enters-attached/you"]));
    assert_eq!(
        reply.text,
        "\
check    script:aura/enters-attached/you
family   script (regression, any platform)
record   9 failures in 74 runs; median cost 0.79 billion
state    on
cache    no result for these inputs
runs     in the gate: score 0.1266, threshold 0.004
"
    );
    let reply = verb.run(&world.ctx, &request(&["lint:fmt", "--inputs"]));
    let lines: Vec<&str> = reply.text.lines().collect();
    assert_eq!(lines[0], "dir      src  tree aaaaaaa");
    assert!(lines[1].starts_with("hash     "), "{}", reply.text);
    assert!(
        lines[1].ends_with("passed on m-2 at 1970-01-01 00:00"),
        "{}",
        reply.text
    );
}

#[test]
fn a_check_whose_inputs_cannot_be_resolved_only_affects_runs_that_name_it() {
    let world = jsonl_world();
    *world.launcher.listing.lock().unwrap() =
        "{\"check\": \"unit:a\"}\n{\"check\": \"unit:c\", \"inputs\": [\"missing.txt\"]}\n"
            .to_owned();
    let only_a = run(&world, &["unit:a"]);
    assert!(only_a.passed(), "{only_a:?}");
    assert_eq!(only_a.outcomes.len(), 1);
    let only_c = run(&world, &["unit:c"]);
    assert_eq!(outcome(&only_c, "unit:c"), Outcome::Incomplete);
    assert_eq!(only_c.outcomes.len(), 1);
}

#[test]
fn tip_verification_lists_the_regression_checks_the_hub_has_not_switched_off() {
    let mut history = FakeHistory::default();
    let off = CheckStats {
        check: name("unit:b"),
        family: "unit".to_owned(),
        state: CheckState::Off,
        areas: Vec::new(),
        median_cost: None,
        unit: "cpu_s".to_owned(),
    };
    history.replies.insert(
        "unit".to_owned(),
        CheckStatsReply {
            family: "unit".to_owned(),
            family_rate: 0.0,
            checks: vec![off],
        },
    );
    let manifest = &MANIFEST[..MANIFEST.find("[family.rs]").unwrap()];
    let world = world(manifest, &[], history);
    *world.launcher.listing.lock().unwrap() =
        "{\"check\": \"unit:a\"}\n{\"check\": \"unit:b\"}\n".to_owned();

    let listed = world
        .checks
        .regression_checks(&world.ctx, &TreeId::new("tree-01").unwrap())
        .unwrap();

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].check, name("unit:a"));
    assert_eq!(listed[0].inputs, vec!["src".to_owned()]);
    assert!(listed[0].runnable && listed[0].reuse);
    assert_eq!(listed[0].key.platform, None);
}
