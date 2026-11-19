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
