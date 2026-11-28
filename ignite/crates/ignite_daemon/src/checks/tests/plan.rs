//! The verbs that read the gate's plan and a check's record: `ignite check`, `check --plan`,
//! `why` and the list of checks tip verification runs, against a seeded history.

use ignite_core::protocol::hub::{CheckFix, FixOutcome, FixSource};

use super::*;

/// A history in which the gate's plan reads as the example under "Where regression checks run"
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
        startup_billions: None,
        startup_cpu_s: None,
        family: family.to_owned(),
        family_rate: 0.02,
        runs: None,
        checks,
    };
    let mut history = FakeHistory::default();
    let check = name("script:aura/enters-attached/you");
    history.fixes.insert(
        check.to_string(),
        vec![CheckFix {
            check,
            ticket: "ABCD".parse().unwrap(),
            at: Timestamp::EPOCH,
            outcome: FixOutcome::TestOnly,
            source: FixSource::Regression,
            first_bad: None,
            fixing_commit: None,
        }],
    );
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
             always = {always}\nlist = \"lister\"\nrun = \"runner {{checks}}\"\n\
             inputs = [\"src\", \"game_rules\"]\n"
        )
    };
    let manifest = [
        family("script", false),
        family("rust", false),
        family("lint", true),
    ]
    .join("\n");
    let world = world(&manifest, &["game_rules/tests/entering.rs"], history);
    world
        .git
        .set_object("game_rules", Oid::new("e".repeat(40)).unwrap());
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
    let tree = world
        .checks
        .open(&world.ctx, &TreeId::new("tree-01").unwrap())
        .unwrap()
        .1;
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
            cost: ignite_core::protocol::hub::ResultCost::default(),
            areas: Vec::new(),
        })
        .collect();
    world.hub.store(&results).unwrap();
}

#[test]
fn check_plan_prints_the_documented_example() {
    let world = seeded();
    let verb = CheckVerb {
        checks: world.checks.clone(),
        express: None,
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
budget   60 s of checks at 17.5 billion/s = 1,050 billion
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

fn recent_catch_world() -> World {
    let area = "game_rules/tests/";
    let culprit: Commit = "abc1111".parse().unwrap();
    let unrelated: Commit = "abc2222".parse().unwrap();
    let window = ignite_core::score::RECENT_PRODUCT_CATCH_SECONDS;
    let cases = [
        (
            "recent",
            FixOutcome::Product,
            -window + 60.0,
            &culprit,
            2_000.0,
        ),
        (
            "expired",
            FixOutcome::Product,
            -window - 60.0,
            &culprit,
            2_000.0,
        ),
        ("unrelated", FixOutcome::Product, -60.0, &unrelated, 2_000.0),
        ("test-only", FixOutcome::TestOnly, -60.0, &culprit, 1.0),
        ("no-fix", FixOutcome::None, -60.0, &culprit, 1.0),
    ];
    let mut history = FakeHistory::default();
    let mut stats = Vec::new();
    let mut listing = Vec::new();
    for (check, outcome, at, commit, cost) in cases {
        let check = name(&format!("unit:{check}"));
        history.fixes.insert(
            check.to_string(),
            vec![CheckFix {
                check: check.clone(),
                ticket: "ABCD".parse().unwrap(),
                at: Timestamp::EPOCH.plus_seconds(at),
                outcome,
                source: FixSource::Regression,
                first_bad: Some(commit.clone()),
                fixing_commit: None,
            }],
        );
        listing.push(serde_json::json!({ "check": check }).to_string());
        stats.push(CheckStats {
            check,
            family: "unit".to_owned(),
            state: CheckState::On,
            areas: vec![AreaStats {
                area: area.to_owned(),
                runs: 100,
                failures: 30,
            }],
            median_cost: Some(cost),
            unit: "instructions".to_owned(),
        });
    }
    history.replies.insert(
        "unit".to_owned(),
        CheckStatsReply {
            family: "unit".to_owned(),
            family_rate: 0.30,
            runs: Some(500),
            checks: stats,
            startup_billions: None,
            startup_cpu_s: None,
        },
    );
    let world = world(
        r#"
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
"#,
        &["game_rules/tests/one.rs"],
        history,
    );
    world
        .git
        .set_commit_paths(&culprit, &["game_rules/tests/other.rs"]);
    world
        .git
        .set_commit_paths(&unrelated, &["game_rules/src/other.rs"]);
    *world.launcher.listing.lock().unwrap() = listing.join("\n");
    world
}

#[test]
fn recent_product_catches_run_outside_the_budget_and_other_outcomes_do_not_raise_scores() {
    let world = recent_catch_world();
    let tree = TreeId::new("tree-01").unwrap();
    let gated = world.checks.gate(&world.ctx, &tree, &options(false));
    assert!(gated.run.passed(), "{:?}", gated.run);
    let budget = gated.budget.unwrap();
    assert_eq!(budget.forced, (1, 2_000.0));
    assert!(budget.billions() < budget.forced.1);
    assert_eq!(budget.scored, (0, 0.0));
    assert_eq!(gated.left, 4);
    assert_eq!(
        world.launcher.commands("runner"),
        [vec!["runner".to_owned(), "unit:recent".to_owned()]]
    );

    let reply = CheckVerb {
        checks: world.checks.clone(),
        express: None,
    }
    .run(
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
    assert!(
        reply
            .text
            .contains("recent   unit:recent  product bug in this area"),
        "{}",
        reply.text
    );
    let json = reply.json.unwrap();
    assert_eq!(
        json["recent_product_catches"],
        serde_json::json!(["unit:recent"])
    );
    assert_eq!(json["tip"], 4);
}

#[test]
fn budget_family_product_fixes_do_not_force_or_score_into_the_gate() {
    let mut world = recent_catch_world();
    world.ctx.config.rules.budgets = vec!["unit".to_owned()];
    let tree = TreeId::new("tree-01").unwrap();
    let gated = world.checks.gate(&world.ctx, &tree, &options(false));
    assert!(gated.run.passed(), "{:?}", gated.run);
    let budget = gated.budget.unwrap();
    assert_eq!(budget.forced, (0, 0.0));
    assert_eq!(budget.scored, (0, 0.0));
    assert_eq!(gated.left, 5);
    assert!(world.launcher.commands("runner").is_empty());
}

#[test]
fn check_runs_the_gate_and_answers_the_rest_from_the_cache() {
    let world = seeded();
    let verb = CheckVerb {
        checks: world.checks.clone(),
        express: None,
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
    let lines: Vec<&str> = reply.text.lines().skip(1).collect();
    assert_eq!(
        lines,
        [
            "budget   60 s at 17.5 billion/s = 1,050 billion: 15 checks (94 billion) within it, \
             2 checks (2 billion) outside it, 48 checks (48 billion) cached",
            "build    nothing built",
            "listed   1 s listing 1 family afresh, 2 kept; slowest: rust 1 s",
            "tip      1,204 checks left to tip verification: 1,204 checks (1,204 billion) below \
             the threshold",
            "verdict  stored for this tree",
        ],
        "{}",
        reply.text
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
fix outcomes (30d) 1970-01-01  test-only  ABCD (regression)
cache    no result for these inputs
runs     in the gate: score 0.1266, threshold 0.004
"
    );
    let reply = verb.run(&world.ctx, &request(&["lint:fmt", "--inputs"]));
    let lines: Vec<&str> = reply.text.lines().collect();
    assert_eq!(lines[0], "dir      game_rules  tree eeeeeee");
    assert_eq!(lines[1], "dir      src         tree aaaaaaa");
    assert!(lines[2].starts_with("hash     "), "{}", reply.text);
    assert!(
        lines[2].ends_with("passed on m-2 at 1970-01-01 00:00"),
        "{}",
        reply.text
    );
}

#[test]
fn why_outside_a_tree_prints_the_record() {
    let world = seeded();
    let verb = WhyVerb {
        checks: world.checks.clone(),
    };
    let request = |args: &[&str]| Request {
        verb: "why".to_owned(),
        args: args.iter().map(|s| (*s).to_owned()).collect(),
        // The main checkout, which holds no build tree.
        cwd: ignite_core::test_files::temp_dir().join("ignite-main-checkout"),
        session: None,
        wait_s: 1,
        pid: None,
    };
    let launched = world.launcher.runs.lock().unwrap().len();
    let reply = verb.run(&world.ctx, &request(&["script:aura/enters-attached/you"]));
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert_eq!(
        reply.text,
        "\
check    script:aura/enters-attached/you
family   script (regression, any platform)
record   9 failures in 74 runs; median cost 0.79 billion
state    on
fix outcomes (30d) 1970-01-01  test-only  ABCD (regression)
runs     depends on the change: run ignite why in a build tree to see the gate's choice
"
    );
    // Nothing was listed or built to answer it.
    assert_eq!(world.launcher.runs.lock().unwrap().len(), launched);
    let reply = verb.run(&world.ctx, &request(&["script:s/run-"]));
    assert_eq!(reply.exit, Exit::Usage);
    assert!(reply.text.starts_with("14 checks match"), "{}", reply.text);
    let reply = verb.run(&world.ctx, &request(&["lint:fmt", "--inputs"]));
    assert_eq!(reply.exit, Exit::Usage);
}

#[test]
fn deleting_a_file_matched_by_a_check_glob_forces_only_that_check() {
    let mut world = jsonl_world();
    world.ctx.config.gate.seconds = 0.01;
    *world.launcher.listing.lock().unwrap() = concat!(
        r#"{"check": "unit:uses-fixture", "inputs": ["data/*.json"]}"#,
        "\n",
        r#"{"check": "unit:unrelated"}"#,
        "\n",
    )
    .to_owned();
    world
        .git
        .set_object("data/core.json", Oid::new("a".repeat(40)).unwrap());
    let mut trees = FakeTrees::new(&world.dir, &["data/old fixture.json"]);
    trees.deleted = trees.changed.clone();
    let checks = daemon(
        &world,
        ignite_store::local::open_in_memory().unwrap(),
        trees,
    );
    let verb = CheckVerb {
        checks,
        express: None,
    };
    let request = |args| Request {
        verb: "check".to_owned(),
        args,
        cwd: world.dir.clone(),
        session: None,
        wait_s: 1,
        pid: None,
    };
    let plan = verb.run(&world.ctx, &request(vec!["--plan".to_owned()]));
    assert_eq!(plan.exit, Exit::Success, "{}", plan.text);
    assert!(
        plan.text.contains("an input was deleted by this change"),
        "{}",
        plan.text
    );
    assert_eq!(
        plan.json.unwrap()["run"],
        serde_json::json!(["unit:uses-fixture"])
    );

    let run = verb.run(&world.ctx, &request(Vec::new()));
    assert_eq!(run.exit, Exit::Success, "{}", run.text);
    assert_eq!(
        world.launcher.commands("runner"),
        [vec!["runner".to_owned(), "unit:uses-fixture".to_owned()]]
    );
}

#[test]
fn a_required_deleted_file_is_reported_by_name() {
    let world = jsonl_world();
    *world.launcher.listing.lock().unwrap() =
        r#"{"check": "unit:needs-fixture", "inputs": ["data/fixture.json"]}"#.to_owned();
    let mut trees = FakeTrees::new(&world.dir, &["data/fixture.json"]);
    trees.deleted = trees.changed.clone();
    // An unstaged deletion still appears in the Git index.
    trees.tracked = ["src/a.py", "data/fixture.json"]
        .map(str::to_owned)
        .to_vec();
    let checks = daemon(
        &world,
        ignite_store::local::open_in_memory().unwrap(),
        trees,
    );
    let gated = checks.gate(
        &world.ctx,
        &TreeId::new("tree-01").unwrap(),
        &options(false),
    );

    assert!(!gated.run.passed());
    assert_eq!(
        outcome(&gated.run, "unit:needs-fixture"),
        Outcome::Incomplete
    );
    assert!(
        gated.run.failures[0]
            .message
            .as_deref()
            .unwrap()
            .contains("data/fixture.json")
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
            startup_billions: None,
            startup_cpu_s: None,
            family: "unit".to_owned(),
            family_rate: 0.0,
            runs: None,
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
        .unwrap()
        .checks;

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].check, name("unit:a"));
    assert_eq!(listed[0].inputs, vec!["src".to_owned()]);
    assert!(listed[0].runnable && listed[0].reuse);
    assert_eq!(listed[0].key.platform, None);
}

#[test]
fn an_unreachable_hub_leaves_the_family_unlisted_instead_of_running_switched_off_checks() {
    let mut history = FakeHistory::default();
    history.fail_stats.insert("unit".to_owned());
    let manifest = &MANIFEST[..MANIFEST.find("[family.rs]").unwrap()];
    let world = world(manifest, &[], history);
    *world.launcher.listing.lock().unwrap() =
        "{\"check\": \"unit:a\"}\n{\"check\": \"unit:b\"}\n".to_owned();

    let set = world
        .checks
        .regression_checks(&world.ctx, &TreeId::new("tree-01").unwrap())
        .unwrap();

    assert!(set.checks.is_empty());
    assert!(set.off.is_empty());
    assert_eq!(set.unlisted.len(), 1);
    assert_eq!(set.unlisted[0].family, "unit");
    assert!(!set.unlisted[0].reportable);
    assert!(set.unlisted[0].message.contains("switched off"));
}

/// The hub's estimate of a family's start-up reaches each of its checks, so the gate charges it
/// once for the family; billions of instructions are preferred to CPU seconds.
#[test]
fn a_scored_check_carries_its_familys_start_up() {
    use ignite_core::score::Cost;
    let manifest =
        Manifest::from_files(&[(PathBuf::from("checks/test.toml"), MANIFEST.to_owned())]).unwrap();
    let family = &manifest.families["unit"].family;
    let reply = |billions: Option<f64>, cpu_s: Option<f64>| CheckStatsReply {
        family: "unit".to_owned(),
        family_rate: 0.0,
        runs: None,
        checks: Vec::new(),
        startup_billions: billions,
        startup_cpu_s: cpu_s,
    };
    let startup = |reply: Option<&CheckStatsReply>| {
        crate::checks::plan::scored_check(&name("unit:a"), family, reply, None, None)
            .unwrap()
            .family_startup
    };
    assert_eq!(
        startup(Some(&reply(Some(720.0), Some(42.0)))),
        Cost::Billions(720.0)
    );
    assert_eq!(
        startup(Some(&reply(None, Some(42.0)))),
        Cost::CpuSeconds(42.0)
    );
    assert_eq!(startup(Some(&reply(None, None))), Cost::Billions(0.0));
    assert_eq!(startup(None), Cost::Billions(0.0));
}
