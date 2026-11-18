//! The verbs that read the gate's plan and a check's record: `ignite check`, `check --plan`,
//! `why` and the list of checks tip verification runs, against a seeded history.

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
    let lines: Vec<&str> = reply.text.lines().skip(1).collect();
    assert_eq!(
        lines,
        [
            "budget   60 s at 17.5 billion/s = 1,050 billion: 15 checks (94 billion) within it, \
             2 checks (2 billion) outside it, 48 checks (48 billion) cached",
            "build    nothing built",
            "tip      1,204 checks left to tip verification: 1,204 checks (1,204 billion) below \
             the threshold",
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
        cwd: std::env::temp_dir().join("ignite-main-checkout"),
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
        .unwrap()
        .checks;

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].check, name("unit:a"));
    assert_eq!(listed[0].inputs, vec!["src".to_owned()]);
    assert!(listed[0].runnable && listed[0].reuse);
    assert_eq!(listed[0].key.platform, None);
}
