//! Observed reads: a Rust test's hash covers the files its build was observed to read, with no
//! manifest entry naming them, and no file it did not read.

use super::*;

/// A Rust test family whose product names a data file the tests never read, so that it rebuilds
/// when that file changes.
const REPOSITORY: &str = r#"
[product.rust-tests]
build = "cargo build --tests"
dir = "rust"
inputs = ["crate-closure", "rust/Cargo.toml", "data/built-from.json"]
outputs = ["rust/target"]

[family.rust]
kind = "regression"
adapter = "nextest"
platforms = ["macos"]
needs = ["rust-tests"]
inputs = ["crate-closure", "rust/Cargo.toml"]
"#;

const TRACKED: &[&str] = &[
    "data/built-from.json",
    "data/included.tsv",
    "data/unread.json",
    "rust/Cargo.lock",
    "rust/Cargo.toml",
    "rust/crates/core/src/lib.rs",
];

const METADATA: &str = r#"{"packages": [{"name": "core",
    "manifest_path": "/repo/rust/crates/core/Cargo.toml", "dependencies": []}],
    "workspace_root": "/repo/rust"}"#;

fn oid(c: char) -> Oid {
    Oid::new(c.to_string().repeat(40)).unwrap()
}

/// The world of [`REPOSITORY`], whose tree's target directory holds the dep-info of `core`'s
/// test binary, which includes `data/included.tsv`.
fn repository() -> World {
    let world = world(REPOSITORY, &[], FakeHistory::default());
    for path in TRACKED.iter().copied().chain(["rust/crates/core"]) {
        world.git.set_object(path, oid('a'));
    }
    *world.launcher.listing.lock().unwrap() = NEXTEST_LISTING.to_owned();
    let deps = world.dir.join("rust/target/debug/deps");
    fs::create_dir_all(&deps).unwrap();
    let root = world.dir.display();
    fs::write(
        deps.join("core-1a2b.d"),
        format!(
            "{root}/rust/target/debug/deps/core-1a2b: {root}/rust/crates/core/src/lib.rs \
             {root}/rust/crates/core/src/../../../../data/included.tsv\n"
        ),
    )
    .unwrap();
    world
}

fn tree_with(world: &World, changed: &[&str]) -> FakeTrees {
    let mut trees = FakeTrees::new(&world.dir, changed);
    trees.tracked = TRACKED.iter().map(|s| (*s).to_owned()).collect();
    trees.graph.add_workspace(METADATA, "/repo").unwrap();
    trees
}

/// The key of the family's one test on `checks`, and what its hash covers.
fn keyed(world: &World, checks: &Checks) -> Keyed {
    let tree = checks.open(&TreeId::new("tree-01").unwrap()).unwrap();
    let mut probe = CheckRun::default();
    let prepared = checks.prepare(&world.ctx, &tree, &["rust"], &options(false), &mut probe);
    assert_eq!(prepared.len(), 1, "{probe:?}");
    prepared[0].keyed[0].clone()
}

fn listings_made(world: &World) -> usize {
    world
        .launcher
        .commands("cargo")
        .iter()
        .filter(|args| args[2] == "list")
        .count()
}

#[test]
fn an_included_file_enters_the_hash_and_an_unread_one_does_not() {
    let world = repository();
    let checks = daemon(
        &world,
        ignite_store::local::open_in_memory().unwrap(),
        tree_with(&world, &[]),
    );
    let before = keyed(&world, &checks);
    let paths: Vec<&str> = before.inputs.iter().map(|(p, _)| p.as_str()).collect();
    assert!(paths.contains(&"data/included.tsv"), "{paths:?}");
    // The product's own path input decides when it rebuilds, not what a test read.
    assert!(!paths.contains(&"data/built-from.json"), "{paths:?}");

    world.git.set_object("data/unread.json", oid('b'));
    assert_eq!(keyed(&world, &checks).key, before.key);
    assert_eq!(listings_made(&world), 1, "the kept listing still holds");
    // The listing is made again, since the product's inputs enter the listing hash, but the
    // test's own hash stays.
    world.git.set_object("data/built-from.json", oid('b'));
    assert_eq!(keyed(&world, &checks).key, before.key);
    let listed = listings_made(&world);

    world.git.set_object("data/included.tsv", oid('b'));
    let after = keyed(&world, &checks);
    assert_ne!(after.key, before.key);
    assert_eq!(
        listings_made(&world),
        listed + 1,
        "a listing whose observed files changed is made again"
    );
}

#[test]
fn a_kept_listing_brings_its_observed_reads_to_another_machine() {
    let world = repository();
    let first = daemon(
        &world,
        ignite_store::local::open_in_memory().unwrap(),
        tree_with(&world, &[]),
    );
    let made = keyed(&world, &first);
    // Another machine whose target directory holds nothing finds the listing at the hub.
    fs::remove_dir_all(world.dir.join("rust/target")).unwrap();
    let other = daemon(
        &world,
        ignite_store::local::open_in_memory().unwrap(),
        tree_with(&world, &[]),
    );
    assert_eq!(keyed(&world, &other).key, made.key);
    assert_eq!(listings_made(&world), 1);
}

#[test]
fn a_change_to_an_included_file_keeps_the_family_in_scope() {
    let world = repository();
    let checks = daemon(
        &world,
        ignite_store::local::open_in_memory().unwrap(),
        tree_with(&world, &[]),
    );
    keyed(&world, &checks);
    let families = [("rust", &checks.settings.manifest.families["rust"].family)];
    for (changed, in_scope) in [("data/included.tsv", true), ("data/unread.json", false)] {
        let tree = checks.open(&TreeId::new("tree-01").unwrap()).unwrap();
        let (affected, _) = checks.scope(&world.ctx, &tree, &families, &[changed.to_owned()]);
        assert_eq!(!affected.is_empty(), in_scope, "{changed}");
    }
}
