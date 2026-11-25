//! The runner lists, builds and runs a family as the manifests of the tree it runs in declare
//! it, whatever manifests the runner was made with. Verb processes made their runner from the
//! main checkout's manifests, which can be older or newer than the tree: a landing whose commit
//! moved a family to another `cargo nextest` profile ran the old profile, and nextest ran none
//! of the tests.

use super::*;

fn tree() -> TreeId {
    TreeId::new("tree-01").unwrap()
}

/// The family `unit` as the runner's own manifests declare it: listed by `lister`, needing the
/// product `deps`.
fn held_world() -> World {
    let held = "[product.deps]\nbuild = \"make deps\"\ninputs = [\"src\"]\noutputs = [\"out\"]\n\n\
                [family.unit]\nkind = \"regression\"\nadapter = \"jsonl\"\nplatforms = \"any\"\n\
                needs = [\"deps\"]\nlist = \"lister\"\nrun = \"runner {checks}\"\n\
                inputs = [\"src\"]\n";
    let world = world(held, &["src/a.py"], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() =
        "{\"check\": \"unit:a\"}\n{\"check\": \"unit:b\"}\n".to_owned();
    world
}

/// Writes the tree's own manifests: `unit` lists with another command, is in the always set and
/// needs no product.
fn tree_declares(world: &World, text: &str) {
    let dir = world.dir.join("checks");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("test.toml"), text).unwrap();
}

const TREE_MANIFEST: &str = "[family.unit]\nkind = \"regression\"\nadapter = \"jsonl\"\n\
                             platforms = \"any\"\nalways = true\nlist = \"lister --tree\"\n\
                             run = \"runner {checks}\"\ninputs = [\"src\"]\n";

/// Every listing ran the tree's `list` command, and no product the runner's own manifests name
/// was built.
fn ran_the_trees_family(world: &World) {
    let listings = world.launcher.commands("lister");
    assert!(!listings.is_empty());
    for listing in listings {
        assert_eq!(listing, ["lister", "--tree"]);
    }
    assert!(world.builds.requests().is_empty(), "deps was built");
}

#[test]
fn running_named_checks_uses_the_trees_manifests() {
    let world = held_world();
    tree_declares(&world, TREE_MANIFEST);

    let run = run(&world, &["unit:a"]);

    assert_eq!(outcome(&run, "unit:a"), Outcome::Pass);
    ran_the_trees_family(&world);
}

#[test]
fn ignite_test_uses_the_trees_manifests() {
    let world = held_world();
    tree_declares(&world, TREE_MANIFEST);
    let verb = verbs::TestVerb {
        checks: world.checks.clone(),
        express: None,
    };
    let request = Request {
        verb: "test".to_owned(),
        args: vec!["unit:a".to_owned()],
        cwd: world.dir.join("sub"),
        session: None,
        wait_s: 1,
        pid: None,
    };

    let reply = verb.run(&world.ctx, &request);

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    ran_the_trees_family(&world);
}

#[test]
fn the_landing_gate_uses_the_trees_manifests() {
    let world = held_world();
    tree_declares(&world, TREE_MANIFEST);
    let landing = RunOptions {
        verb: "land".to_owned(),
        class: Class::Land,
        rerun: false,
        ticket: None,
        session: None,
        reserve: None,
    };

    let gated = world.checks.gate(&world.ctx, &tree(), &landing);

    // The tree puts `unit` in the always set, which the runner's own manifests do not.
    assert!(gated.run.passed(), "{:?}", gated.run.failures);
    assert_eq!(outcome(&gated.run, "unit:a"), Outcome::Pass);
    assert_eq!(outcome(&gated.run, "unit:b"), Outcome::Pass);
    ran_the_trees_family(&world);
}

/// Manifests the tree has but that do not parse leave its checks incomplete; running the
/// runner's own manifests on the tree's sources instead is what reading the tree's prevents.
#[test]
fn manifests_the_tree_cannot_parse_run_nothing() {
    let world = held_world();
    tree_declares(&world, "[family.unit\n");

    let run = run(&world, &["unit:a"]);

    assert_eq!(outcome(&run, "unit:a"), Outcome::Incomplete);
    let message = run.failures[0].message.clone().unwrap_or_default();
    assert!(message.contains("the manifests of tree"), "{message}");
    assert!(world.launcher.commands("lister").is_empty());
}

/// The `ignite.toml` of a tree that declares this machine (`local`) with `families`, which is
/// the text of a TOML `families = ...` line or empty for none.
fn tree_config(world: &World, families: &str) {
    let text = format!(
        "[repository]\nremote = \"origin\"\nstaging = \"staging\"\nmaster = \"master\"\n\
         trees = \"trees\"\nchecks = \"checks\"\n[hub]\nurl = \"http://localhost:1\"\n\
         project = \"test\"\n[gate]\nthreshold = 0.004\nseconds = 60\n\
         [machine.local]\nticket_trees = 1\nlanding_trees = 1\n{families}\n"
    );
    fs::write(world.dir.join("ignite.toml"), text).unwrap();
}

/// A runner made as a daemon makes it when its main checkout's `ignite.toml` lists no family
/// for this machine: the family `unit` is declared, but this machine does not run it.
fn runs_no_family(world: &World) -> Arc<Checks> {
    let mut settings = world.checks.settings.clone();
    settings.families = Some(Vec::new());
    Arc::new(Checks::new(
        settings,
        ResultCache::new(
            ignite_store::local::open_in_memory().unwrap(),
            world.hub.clone(),
            world.ctx.machine.clone(),
            world.ctx.clock.clone(),
        ),
        Box::new(FakeTrees::new(&world.dir, &["src/a.py"])),
        Box::new(FakeHistory::default()),
    ))
}

/// The checks a tip round lists at the tree that this machine runs itself.
fn runnable_here(checks: &Checks, world: &World) -> Vec<String> {
    let set = checks.regression_checks(&world.ctx, &tree()).unwrap();
    let mut names: Vec<String> = set
        .checks
        .iter()
        .filter(|check| check.runnable)
        .map(|check| check.check.to_string())
        .collect();
    names.sort();
    names
}

/// Tip verification once waited about 15 hours on a family that no machine ran, because the
/// change that added it to a machine's `families` could not reach `master`, and each daemon
/// read its list only from its main checkout (which follows `master`) when it started. A round
/// now takes the list from the `ignite.toml` of the commit it verifies, so the change that adds
/// the family has the machine run it in the same round, with no restart.
#[test]
fn a_tip_whose_ignite_toml_adds_a_family_to_this_machine_runs_it() {
    let world = held_world();
    let checks = runs_no_family(&world);
    assert!(
        runnable_here(&checks, &world).is_empty(),
        "with no ignite.toml in the tree, the daemon's own list applies"
    );

    tree_config(&world, "families = [\"unit\"]");

    assert_eq!(runnable_here(&checks, &world), ["unit:a", "unit:b"]);
    let batches = [(0, vec![name("unit:a"), name("unit:b")])];
    let (run, ran) = checks.run_batches(
        &world.ctx,
        &tree(),
        "unit",
        &batches,
        &options(false),
        &super::super::BatchCallbacks {
            claim: &|_| true,
            failed: &|_| {},
            stop: None,
        },
    );
    assert_eq!(ran, 1, "{:?}", run.failures);
    assert_eq!(outcome(&run, "unit:a"), Outcome::Pass);
    assert_eq!(outcome(&run, "unit:b"), Outcome::Pass);
}

/// A tree that declares this machine with no `families` list has it run every family; one whose
/// `ignite.toml` does not parse keeps the list the daemon read, as the express lane keeps its
/// commands.
#[test]
fn a_tree_without_a_list_runs_every_family_and_one_that_does_not_parse_keeps_the_daemons() {
    let world = held_world();
    let checks = runs_no_family(&world);

    tree_config(&world, "");
    assert_eq!(runnable_here(&checks, &world), ["unit:a", "unit:b"]);

    fs::write(world.dir.join("ignite.toml"), "this is = not [valid").unwrap();
    assert!(runnable_here(&checks, &world).is_empty());
}

/// The build phase uses the candidate's classification for both listing and running.
#[test]
fn a_tip_uses_the_candidates_build_families_and_reserves_their_cores() {
    use ignite_core::traits::Reserve;

    use crate::tip::ListingPhase;

    let mut world = held_world();
    world.ctx.config.repository.green = Some("green".to_owned());
    let manifest = format!(
        "{}\n[family.other]\nkind = \"regression\"\nadapter = \"jsonl\"\nplatforms = \"any\"\n         list = \"lister --other\"\nrun = \"runner {{checks}}\"\ninputs = [\"src\"]\n",
        TREE_MANIFEST.replace("always = true", "builds = true")
    );
    tree_declares(&world, &manifest);
    *world.launcher.listing.lock().unwrap() =
        "{\"check\":\"unit:a\"}\n{\"check\":\"other:b\"}\n".to_owned();

    let builds = world
        .checks
        .regression_checks_for(&world.ctx, &tree(), ListingPhase::Build)
        .unwrap();
    assert_eq!(builds.checks.len(), 1);
    assert_eq!(builds.checks[0].check, name("unit:a"));
    assert!(builds.checks[0].is_build());
    assert_eq!(
        world.launcher.commands("lister"),
        [vec!["lister", "--tree"]]
    );

    let tip = RunOptions {
        verb: "tip".to_owned(),
        class: Class::Tip,
        ..options(false)
    };
    let (run, ran) = world.checks.run_batches(
        &world.ctx,
        &tree(),
        "unit",
        &[(0, vec![name("unit:a")])],
        &tip,
        &super::super::BatchCallbacks {
            claim: &|_| true,
            failed: &|_| {},
            stop: None,
        },
    );
    assert_eq!(ran, 1);
    assert_eq!(outcome(&run, "unit:a"), Outcome::Pass);

    let other = world
        .checks
        .regression_checks_for(&world.ctx, &tree(), ListingPhase::Other)
        .unwrap();
    assert_eq!(other.checks.len(), 1);
    assert_eq!(other.checks[0].check, name("other:b"));
    let specs = world.launcher.runs.lock().unwrap();
    let commands: Vec<_> = specs
        .iter()
        .map(|spec| (spec.args[1].as_str(), spec.reserve))
        .collect();
    assert_eq!(
        commands,
        [
            ("--tree", Some(Reserve::TipBuild)),
            ("unit:a", Some(Reserve::TipBuild)),
            ("--other", Some(Reserve::TipRound))
        ]
    );
}
