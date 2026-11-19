//! The gate's scope: the families a change cannot affect are left out before anything is
//! listed or built, and the builds a command starts run in its class.

use super::*;
use crate::checks::Carried;

/// A repository shaped like the one Ignite was built for: a Rust test family, a family that
/// needs a native client's build, and an always-run family whose checks read the docs.
const REPOSITORY: &str = r#"
[product.rust-tests]
build = "cargo build --tests"
dir = "rust"
inputs = ["crate-closure", "rust/Cargo.toml"]
outputs = ["rust/target"]

[product.client]
build = "client build"
inputs = ["rust/client/**", "rust/crates/**"]
outputs = ["rust/client/target"]

[family.rust]
kind = "regression"
adapter = "nextest"
platforms = ["macos"]
needs = ["rust-tests"]
inputs = ["crate-closure", "rust/Cargo.toml"]

[family.client-interface]
kind = "regression"
adapter = "jsonl"
platforms = ["macos"]
needs = ["client"]
list = "lister client-interface"
run = "runner {checks}"
inputs = ["crate:core", "data/client/**"]

[family.markdown]
kind = "regression"
adapter = "jsonl"
platforms = "any"
always = true
list = "lister markdown"
run = "runner {checks}"
inputs = ["tools/rules.py"]
"#;

const TRACKED: &[&str] = &[
    "docs/ignite.md",
    "data/client/layout.json",
    "rust/Cargo.lock",
    "rust/Cargo.toml",
    "rust/client/src/main.rs",
    "rust/crates/core/src/lib.rs",
    "tools/rules.py",
];

/// `cargo metadata` for one workspace package, `core`, in `rust/`.
const METADATA: &str = r#"{"packages": [{"name": "core",
    "manifest_path": "/repo/rust/crates/core/Cargo.toml", "dependencies": []}],
    "workspace_root": "/repo/rust"}"#;

/// The world of [`REPOSITORY`], whose object IDs start from `a`.
fn repository() -> World {
    let world = world(REPOSITORY, &[], FakeHistory::default());
    for (path, c) in [
        ("docs", 'a'),
        ("docs/ignite.md", 'a'),
        ("data/client", 'a'),
        ("rust/Cargo.lock", 'a'),
        ("rust/Cargo.toml", 'a'),
        ("rust/client", 'a'),
        ("rust/crates", 'a'),
        ("rust/crates/core", 'a'),
        ("tools/rules.py", 'a'),
    ] {
        world
            .git
            .set_object(path, Oid::new(c.to_string().repeat(40)).unwrap());
    }
    *world.launcher.listing.lock().unwrap() = format!(
        "{{\"check\": \"client-interface:hover\"}}\n\
         {{\"check\": \"markdown:format\", \"inputs\": [\"docs/**\"]}}\n{NEXTEST_LISTING}\n"
    );
    world
}

/// The tree of [`repository`], changed in `changed`.
fn tree_with(world: &World, changed: &[&str]) -> FakeTrees {
    let mut trees = FakeTrees::new(&world.dir, changed);
    trees.tracked = TRACKED.iter().map(|s| (*s).to_owned()).collect();
    trees.graph.add_workspace(METADATA, "/repo").unwrap();
    trees
}

fn memory() -> rusqlite::Connection {
    ignite_store::local::open_in_memory().unwrap()
}

/// Lists every family of `world` on a daemon of its own, as tip verification does.
fn listed_at_the_tip(world: &World) {
    let tip = daemon(world, memory(), tree_with(world, &[]));
    let tree = tip.open(&TreeId::new("tree-01").unwrap()).unwrap();
    let mut probe = CheckRun::default();
    let families = ["client-interface", "markdown", "rust"];
    let prepared = tip.prepare(&world.ctx, &tree, &families, &options(false), &mut probe);
    assert_eq!(prepared.len(), 3, "{:?}", probe.incomplete_families);
}

fn landing() -> RunOptions {
    RunOptions {
        verb: "land".to_owned(),
        class: Class::Land,
        rerun: false,
        ticket: Some("tk.7".parse().unwrap()),
        session: None,
    }
}

fn list_commands(world: &World) -> usize {
    world.launcher.commands("lister").len()
        + world
            .launcher
            .commands("cargo")
            .iter()
            .filter(|args| args[2] == "list")
            .count()
}

/// A landing that changed only `docs/ignite.md` compiled Rust test programs for over 35
/// minutes, because the gate listed every family, building what each listing needs, before it
/// asked which checks the change affects. It now asks first, from the listings tip
/// verification kept, on a daemon that has just started.
#[test]
fn a_change_to_the_docs_lists_no_rust_family_and_builds_nothing() {
    let world = repository();
    listed_at_the_tip(&world);
    let builds = world.builds.requests().len();
    let lists = list_commands(&world);

    let restarted = daemon(&world, memory(), tree_with(&world, &["docs/ignite.md"]));
    let gated = restarted.gate(&world.ctx, &TreeId::new("tree-01").unwrap(), &landing());

    assert_eq!(gated.run.out_of_scope, ["client-interface", "rust"]);
    assert_eq!(world.builds.requests().len(), builds, "nothing was built");
    assert_eq!(list_commands(&world), lists, "nothing was listed");
    let ran: Vec<&str> = gated.run.outcomes.iter().map(|(c, _)| c.as_str()).collect();
    assert_eq!(ran, ["markdown:format"], "the always set reads the docs");
    let text = verbs::render(&verbs::gate_lines(&gated));
    assert!(
        text.contains("scope    2 families left out: the change touches none of their inputs"),
        "{text}"
    );
}

#[test]
fn a_change_to_one_crate_lists_only_the_families_that_read_it() {
    let world = repository();
    listed_at_the_tip(&world);
    world
        .git
        .set_object("rust/crates/core", Oid::new("b".repeat(40)).unwrap());
    world
        .git
        .set_object("rust/crates", Oid::new("b".repeat(40)).unwrap());
    let lists = world.launcher.commands("lister").len();

    let changed = ["rust/crates/core/src/lib.rs"];
    let restarted = daemon(&world, memory(), tree_with(&world, &changed));
    let gated = restarted.gate(&world.ctx, &TreeId::new("tree-01").unwrap(), &landing());

    assert_eq!(gated.run.out_of_scope, ["markdown"]);
    let listed: Vec<String> = world.launcher.commands("lister")[lists..]
        .iter()
        .map(|args| args[1].clone())
        .collect();
    assert_eq!(
        listed,
        ["client-interface"],
        "its inputs changed, so it is listed"
    );
}

/// A landing that lost the race for the tip gated its whole change again. A later pass now
/// judges again only the families whose inputs the commits landed since touch, and carries the
/// others over from the pass that passed.
#[test]
fn a_later_pass_judges_only_the_families_the_landed_commits_touch() {
    let world = repository();
    listed_at_the_tip(&world);
    world
        .git
        .set_object("rust/crates/core", Oid::new("b".repeat(40)).unwrap());
    world
        .git
        .set_object("rust/crates", Oid::new("b".repeat(40)).unwrap());
    let changed = ["rust/crates/core/src/lib.rs"];
    let tree = TreeId::new("tree-01").unwrap();
    let first = daemon(&world, memory(), tree_with(&world, &changed));
    let gated = first.gate(&world.ctx, &tree, &landing());
    assert_eq!(gated.scoped, ["client-interface", "rust"]);
    assert!(gated.carried.is_empty());

    // What landed since changed the client's data, which only `client-interface` reads.
    let cargo_lists = || {
        world
            .launcher
            .commands("cargo")
            .iter()
            .filter(|args| args[2] == "list")
            .count()
    };
    let rust_lists = cargo_lists();
    let carried = Carried {
        families: gated.scoped.clone(),
        moved: vec!["data/client/layout.json".to_owned()],
    };
    let again = first.gate_carrying(&world.ctx, &tree, &landing(), Some(&carried));
    assert_eq!(again.scoped, ["client-interface", "rust"]);
    assert_eq!(again.carried, ["rust"]);
    assert!(
        again
            .run
            .outcomes
            .iter()
            .all(|(check, _)| check.family() == "client-interface"),
        "{:?}",
        again.run.outcomes
    );
    assert_eq!(cargo_lists(), rust_lists, "the rust family was not listed");
    let text = verbs::render(&verbs::gate_lines(&again));
    assert!(
        text.contains("carried  1 family from the earlier pass"),
        "{text}"
    );

    // What landed since touched no input of either family: nothing is judged at all.
    let builds = world.builds.requests().len();
    let lists = list_commands(&world);
    let carried = Carried {
        families: gated.scoped.clone(),
        moved: vec!["tools/unread.py".to_owned()],
    };
    let none = first.gate_carrying(&world.ctx, &tree, &landing(), Some(&carried));
    assert_eq!(none.carried, ["client-interface", "rust"]);
    assert!(none.run.outcomes.is_empty());
    assert!(none.run.passed());
    assert_eq!(list_commands(&world), lists, "nothing was listed");
    assert_eq!(world.builds.requests().len(), builds, "nothing was built");
}

#[test]
fn a_family_with_no_kept_listing_is_listed() {
    let world = repository();
    let fresh = daemon(&world, memory(), tree_with(&world, &["docs/ignite.md"]));
    let gated = fresh.gate(&world.ctx, &TreeId::new("tree-01").unwrap(), &landing());
    assert!(gated.run.out_of_scope.is_empty());
    assert_eq!(list_commands(&world), 3, "every family is listed");
    // The listings are kept, so the next landing can leave them out.
    let next = daemon(&world, memory(), tree_with(&world, &["docs/ignite.md"]));
    let gated = next.gate(&world.ctx, &TreeId::new("tree-01").unwrap(), &landing());
    assert_eq!(gated.run.out_of_scope, ["client-interface", "rust"]);
}

/// A change confined to Ignite's own files (the express paths) selected 637 checks, the game's
/// families among them, because a family with no kept listing is always listed and a fresh
/// tree has none. Here `docs/**` stands for the express paths: with the change inside them, a
/// family without a kept listing is judged by its declared inputs, and only the always-set
/// family is listed. A change that leaves the express paths lists every family again.
#[test]
fn a_change_inside_the_express_paths_lists_no_family_it_does_not_name() {
    let mut world = repository();
    world.ctx.config.express = Some(ignite_core::config::Express {
        paths: vec!["docs/**".to_owned()],
        tree: "express".into(),
        dir: "ignite".to_owned(),
        commands: vec!["true".to_owned()],
        families: Vec::new(),
    });
    let tree = TreeId::new("tree-01").unwrap();
    let fresh = daemon(&world, memory(), tree_with(&world, &["docs/ignite.md"]));
    let gated = fresh.gate(&world.ctx, &tree, &landing());
    assert_eq!(gated.run.out_of_scope, ["client-interface", "rust"]);
    assert_eq!(list_commands(&world), 1, "only the always-set family");

    let mixed = daemon(
        &world,
        memory(),
        tree_with(&world, &["docs/ignite.md", "rust/Cargo.toml"]),
    );
    let gated = mixed.gate(&world.ctx, &tree, &landing());
    assert!(gated.run.out_of_scope.is_empty());
}

/// The builds a landing's listings needed ran in the agents' class with no ticket, so the
/// landing showed no work while they compiled. They now run as the command that needs them.
#[test]
fn a_landings_builds_run_in_its_class_for_its_ticket() {
    let world = repository();
    let landing_tree = daemon(&world, memory(), tree_with(&world, &["rust/Cargo.toml"]));
    landing_tree.gate(&world.ctx, &TreeId::new("tree-01").unwrap(), &landing());
    let builds = world.builds.requests_for();
    assert!(!builds.is_empty());
    for (product, who) in builds {
        assert_eq!(who.class, Class::Land, "{product}");
        assert_eq!(who.ticket, Some("tk.7".parse().unwrap()), "{product}");
    }
}

/// `cargo metadata` for a second workspace, `tool/`, with one package, `hub`, which no family
/// of [`REPOSITORY`] reads.
const TOOL_METADATA: &str = r#"{"packages": [{"name": "hub",
    "manifest_path": "/repo/tool/crates/hub/Cargo.toml", "dependencies": []}],
    "workspace_root": "/repo/tool"}"#;

/// The tree of [`repository`] with the `tool/` workspace beside `rust/`, changed in `changed`.
fn two_workspaces(world: &World, changed: &[&str]) -> FakeTrees {
    let mut trees = tree_with(world, changed);
    trees.tracked.extend(
        [
            "tool/Cargo.lock",
            "tool/Cargo.toml",
            "tool/crates/hub/src/lib.rs",
        ]
        .iter()
        .map(|s| (*s).to_owned()),
    );
    trees.graph.add_workspace(TOOL_METADATA, "/repo").unwrap();
    trees
}

/// A landing that changed only Ignite's own crates listed, built and ran the game's Rust tests.
/// `crate-closure` stood for every package of every workspace in the checkout, both in the
/// listing hash, so the kept listing missed, and in the scope, so the family stayed in it. It
/// now stands for the packages of the family's own workspace and those its checks belong to.
#[test]
fn a_change_to_another_workspaces_crate_leaves_the_rust_families_out() {
    let world = repository();
    for path in ["tool/Cargo.lock", "tool/Cargo.toml", "tool/crates/hub"] {
        world
            .git
            .set_object(path, Oid::new("a".repeat(40)).unwrap());
    }
    let tip = daemon(&world, memory(), two_workspaces(&world, &[]));
    let tree = tip.open(&TreeId::new("tree-01").unwrap()).unwrap();
    let mut probe = CheckRun::default();
    let families = ["client-interface", "markdown", "rust"];
    let prepared = tip.prepare(&world.ctx, &tree, &families, &options(false), &mut probe);
    assert_eq!(prepared.len(), 3, "{:?}", probe.incomplete_families);
    // The change edits the `hub` crate.
    world
        .git
        .set_object("tool/crates/hub", Oid::new("b".repeat(40)).unwrap());
    let builds = world.builds.requests().len();
    let lists = list_commands(&world);

    let changed = ["tool/crates/hub/src/lib.rs"];
    let restarted = daemon(&world, memory(), two_workspaces(&world, &changed));
    let gated = restarted.gate(&world.ctx, &TreeId::new("tree-01").unwrap(), &landing());

    assert_eq!(
        gated.run.out_of_scope,
        ["client-interface", "markdown", "rust"]
    );
    assert_eq!(world.builds.requests().len(), builds, "nothing was built");
    assert_eq!(list_commands(&world), lists, "nothing was listed");
}

/// [`REPOSITORY`] with a family whose checks carry a digest computed from the client's build
/// and the files under `data/oracle/`, as the oracle family's are.
fn with_digests() -> World {
    let manifest = format!(
        "{REPOSITORY}
[family.oracle]
kind = \"regression\"
adapter = \"jsonl\"
platforms = \"any\"
needs = [\"client\"]
list = \"lister oracle\"
run = \"runner {{checks}}\"
inputs = [\"tools/oracle.py\"]
digest_inputs = [\"data/oracle/**\"]
"
    );
    let world = world(&manifest, &[], FakeHistory::default());
    let base = repository();
    for path in ["docs", "docs/ignite.md", "data/client", "rust/Cargo.lock"] {
        world
            .git
            .set_object(path, Oid::new("a".repeat(40)).unwrap());
    }
    for path in [
        "rust/Cargo.toml",
        "rust/client",
        "rust/crates",
        "rust/crates/core",
        "tools/rules.py",
        "tools/oracle.py",
        "data/oracle",
        "data/oracle/gaps.json",
    ] {
        world
            .git
            .set_object(path, Oid::new("a".repeat(40)).unwrap());
    }
    let listing = base.launcher.listing.lock().unwrap().clone();
    *world.launcher.listing.lock().unwrap() =
        format!("{{\"check\": \"oracle:scenario/xmage\", \"digest\": \"d1\"}}\n{listing}");
    world
}

/// The tree of [`with_digests`], changed in `changed`.
fn digest_tree(world: &World, changed: &[&str]) -> FakeTrees {
    let mut trees = tree_with(world, changed);
    trees.tracked.extend(
        ["tools/oracle.py", "data/oracle/gaps.json"]
            .iter()
            .map(|s| (*s).to_owned()),
    );
    trees
}

/// Every landing listed the oracle family, after building what its listing needs, and counted
/// it in scope, because its checks carry digests and a listing with digests was never kept. A
/// family that declares the files its digests come from is judged by them.
#[test]
fn a_family_with_declared_digest_inputs_is_left_out_when_none_changed() {
    let world = with_digests();
    let tip = daemon(&world, memory(), digest_tree(&world, &[]));
    let tree = tip.open(&TreeId::new("tree-01").unwrap()).unwrap();
    let mut probe = CheckRun::default();
    let families = ["client-interface", "markdown", "oracle", "rust"];
    let prepared = tip.prepare(&world.ctx, &tree, &families, &options(false), &mut probe);
    assert_eq!(prepared.len(), 4, "{:?}", probe.incomplete_families);
    let lists = list_commands(&world);
    let builds = world.builds.requests().len();

    let docs = daemon(&world, memory(), digest_tree(&world, &["docs/ignite.md"]));
    let gated = docs.gate(&world.ctx, &TreeId::new("tree-01").unwrap(), &landing());
    assert_eq!(
        gated.run.out_of_scope,
        ["client-interface", "oracle", "rust"]
    );
    assert_eq!(list_commands(&world), lists, "nothing was listed");
    assert_eq!(world.builds.requests().len(), builds, "nothing was built");

    // A change to a file its digests are computed from puts it in scope, and lists it again.
    world
        .git
        .set_object("data/oracle/gaps.json", Oid::new("b".repeat(40)).unwrap());
    world
        .git
        .set_object("data/oracle", Oid::new("b".repeat(40)).unwrap());
    let listers = world.launcher.commands("lister").len();
    let changed = ["data/oracle/gaps.json"];
    let gaps = daemon(&world, memory(), digest_tree(&world, &changed));
    let gated = gaps.gate(&world.ctx, &TreeId::new("tree-01").unwrap(), &landing());
    assert!(
        !gated.run.out_of_scope.contains(&"oracle".to_owned()),
        "{:?}",
        gated.run.out_of_scope
    );
    let listed: Vec<String> = world.launcher.commands("lister")[listers..]
        .iter()
        .map(|args| args[1].clone())
        .collect();
    assert_eq!(listed, ["oracle"]);
}
