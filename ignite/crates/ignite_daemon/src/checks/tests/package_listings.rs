//! A change to one card file built eight products in the landing gate only to list families
//! whose checks it could not change. A family that sets `listing_ignores_comments` keeps its
//! listing across a change that only edits comments, and a `nextest` family that misses its
//! kept listing reuses its newest listing for every package whose test names cannot have
//! changed, listing and building only the others.

use super::*;

const PACKAGES: &str = r#"
[product.deps]
build = "make deps"
inputs = ["game"]
outputs = ["out"]

[family.unit]
kind = "regression"
adapter = "jsonl"
platforms = "any"
needs = ["deps"]
list_needs = []
list = "lister"
run = "runner {checks}"
inputs = ["game"]
listing_ignores_comments = true

[family.rs]
kind = "regression"
adapter = "nextest"
platforms = ["macos"]
needs = ["deps"]
list = "cargo nextest list {packages} --message-format json"
run = "cargo nextest run {packages} --message-format libtest-json-plus"
inputs = ["crate-closure"]
listing_ignores_comments = true
"#;

const CARDS_V1: &str = "//! The cards.\npub fn exile() {}\n";
/// [`CARDS_V1`] with another comment and layout, and no other change.
const CARDS_COMMENTED: &str = "//! The cards, each in a file.\n\n/// Exiles.\npub fn exile() { }\n";
/// [`CARDS_V1`] with a change to its code.
const CARDS_CODE: &str = "//! The cards.\npub fn exile() { let _ = 1; }\n";

/// A listing of both packages' tests, `core::a` and `cards::b`, after the `unit` family's.
fn both_packages() -> String {
    format!(
        "{{\"check\": \"unit:a\"}}\n{}\n",
        r#"{"rust-suites": {"core": {"binary-id": "core", "binary-name": "core", "package-name": "core", "testcases": {"a": {"ignored": false, "filter-match": {"status": "matches"}}}}, "cards": {"binary-id": "cards", "binary-name": "cards", "package-name": "cards", "testcases": {"b": {"ignored": false, "filter-match": {"status": "matches"}}}}}}"#
    )
}

/// A listing of the `cards` package alone, with a test it gained, `cards::c`.
fn cards_only() -> String {
    format!(
        "{{\"check\": \"unit:a\"}}\n{}\n",
        r#"{"rust-suites": {"cards": {"binary-id": "cards", "binary-name": "cards", "package-name": "cards", "testcases": {"b": {"ignored": false, "filter-match": {"status": "matches"}}, "c": {"ignored": false, "filter-match": {"status": "matches"}}}}}}"#
    )
}

/// A successful nextest listing of a package with no test binaries.
const NO_PACKAGE_TESTS: &str =
    "{\"check\": \"unit:a\"}\n{\"test-count\": 0, \"rust-suites\": {}}\n";

/// Writes the workspace with `cards` as the cards package's source, and gives git an object ID
/// for every file and directory, from `version` for those the cards file changes.
fn write_workspace(world: &World, cards: &str, version: char) {
    let files = [
        ("game/Cargo.toml", "[workspace]\n"),
        ("game/Cargo.lock", "version = 4\n"),
        ("game/core/Cargo.toml", "[package]\n"),
        ("game/core/src/lib.rs", "pub fn core() {}\n"),
        ("game/cards/Cargo.toml", "[package]\n"),
        ("game/cards/src/lib.rs", cards),
    ];
    for (path, text) in files {
        let file = world.dir.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
        let c = if path == "game/cards/src/lib.rs" {
            version
        } else {
            'a'
        };
        world
            .git
            .set_object(path, Oid::new(c.to_string().repeat(40)).unwrap());
    }
    for (dir, c) in [
        ("game", version),
        ("game/cards", version),
        ("game/core", 'a'),
    ] {
        world
            .git
            .set_object(dir, Oid::new(c.to_string().repeat(40)).unwrap());
    }
}

fn workspace_world() -> (World, Arc<Checks>) {
    let world = world(PACKAGES, &[], FakeHistory::default());
    write_workspace(&world, CARDS_V1, 'b');
    *world.launcher.listing.lock().unwrap() = both_packages();
    let package = |name: &str, deps: &[&str]| ignite_core::inputs::Package {
        name: name.to_owned(),
        dir: format!("game/{name}"),
        deps: deps.iter().map(|d| (*d).to_owned()).collect(),
        workspace: "game".to_owned(),
    };
    let mut trees = FakeTrees::new(&world.dir, &[]);
    trees.tracked = [
        "game/Cargo.toml",
        "game/Cargo.lock",
        "game/core/Cargo.toml",
        "game/core/src/lib.rs",
        "game/cards/Cargo.toml",
        "game/cards/src/lib.rs",
    ]
    .map(str::to_owned)
    .to_vec();
    trees.graph = CrateGraph {
        packages: vec![package("core", &[]), package("cards", &["core"])],
    };
    let checks = daemon(
        &world,
        ignite_store::local::open_in_memory().unwrap(),
        trees,
    );
    (world, checks)
}

/// Lists and keys every family as the landing gate does, and returns the checks it found.
fn gate_listing(world: &World, checks: &Checks) -> Vec<String> {
    let tree = checks
        .open(&world.ctx, &TreeId::new("tree-01").unwrap())
        .unwrap()
        .1;
    let among: Vec<_> = checks.families().collect();
    let mut run = CheckRun::default();
    let cold = |_: &str, _: &ignite_core::manifest::Family| false;
    let prepared = checks.prepare_gate(&world.ctx, &tree, &among, &options(false), &mut run, &cold);
    assert!(run.incomplete_families.is_empty(), "{run:?}");
    let mut names: Vec<String> = prepared
        .iter()
        .flat_map(|p| p.keyed.iter().map(|k| k.listed.check.as_str().to_owned()))
        .collect();
    names.sort();
    names
}

fn list_commands(world: &World) -> Vec<Vec<String>> {
    let runs = world.launcher.runs.lock().unwrap();
    runs.iter()
        .filter(|s| s.args[0] == "lister" || (s.args[0] == "cargo" && s.args[2] == "list"))
        .map(|s| s.args.clone())
        .collect()
}

#[test]
fn a_change_to_comments_alone_lists_nothing_and_builds_nothing() {
    let (world, checks) = workspace_world();
    let first = gate_listing(&world, &checks);
    assert_eq!(first, ["rs:cards::b", "rs:core::a", "unit:a"]);
    let (listed, built) = (list_commands(&world).len(), world.builds.requests().len());

    write_workspace(&world, CARDS_COMMENTED, 'c');
    assert_eq!(gate_listing(&world, &checks), first);
    assert_eq!(list_commands(&world).len(), listed, "nothing was listed");
    assert_eq!(world.builds.requests().len(), built, "nothing was built");

    // A change to the code lists the `jsonl` family again, which cannot be listed by package.
    write_workspace(&world, CARDS_CODE, 'd');
    gate_listing(&world, &checks);
    let lists = list_commands(&world);
    assert!(
        lists[listed..].iter().any(|args| args[0] == "lister"),
        "{lists:?}"
    );
}

#[test]
fn a_change_to_one_package_lists_and_builds_that_package_alone() {
    let (world, checks) = workspace_world();
    gate_listing(&world, &checks);
    let (listed, built) = (list_commands(&world).len(), world.builds.requests().len());

    write_workspace(&world, CARDS_CODE, 'd');
    *world.launcher.listing.lock().unwrap() = cards_only();
    let names = gate_listing(&world, &checks);
    assert_eq!(
        names,
        ["rs:cards::b", "rs:cards::c", "rs:core::a", "unit:a"],
        "the cards package's tests are listed again, the core package's kept"
    );
    let cargo: Vec<_> = list_commands(&world)[listed..]
        .iter()
        .filter(|args| args[0] == "cargo")
        .cloned()
        .collect();
    assert_eq!(cargo.len(), 1, "{cargo:?}");
    assert_eq!(&cargo[0][3..5], ["-p", "cards"]);
    let requests = world.builds.requests();
    let new: Vec<&Vec<String>> = requests[built..].iter().map(|r| &r.2).collect();
    assert_eq!(
        new,
        [&vec!["cards".to_owned()]],
        "only the cards package was built"
    );

    // The merged listing is kept: the same tree lists nothing again.
    let listed = list_commands(&world).len();
    assert_eq!(gate_listing(&world, &checks), names);
    assert_eq!(list_commands(&world).len(), listed);
}

#[test]
fn a_package_with_no_tests_leaves_the_other_packages_listed() {
    let (world, checks) = workspace_world();
    assert_eq!(
        gate_listing(&world, &checks),
        ["rs:cards::b", "rs:core::a", "unit:a"]
    );
    let listed = list_commands(&world).len();

    write_workspace(&world, CARDS_CODE, 'd');
    *world.launcher.listing.lock().unwrap() = NO_PACKAGE_TESTS.to_owned();
    let names = gate_listing(&world, &checks);
    assert_eq!(names, ["rs:core::a", "unit:a"]);
    let lists = list_commands(&world);
    assert!(
        lists[listed..]
            .iter()
            .any(|args| args.windows(2).any(|pair| pair == ["-p", "cards"]))
    );

    // The empty package result is merged and kept, so it need not be listed again.
    assert_eq!(gate_listing(&world, &checks), names);
    assert_eq!(list_commands(&world).len(), lists.len());
}

#[test]
fn a_merge_with_no_tests_checks_the_whole_family() {
    let (world, checks) = workspace_world();
    *world.launcher.listing.lock().unwrap() = cards_only();
    assert_eq!(
        gate_listing(&world, &checks),
        ["rs:cards::b", "rs:cards::c", "unit:a"]
    );
    let listed = list_commands(&world).len();

    write_workspace(&world, CARDS_CODE, 'd');
    *world.launcher.listing.lock().unwrap() = NO_PACKAGE_TESTS.to_owned();
    let tree = TreeId::new("tree-01").unwrap();
    let set = checks.regression_checks(&world.ctx, &tree).unwrap();
    assert_eq!(set.unlisted.len(), 1);
    assert_eq!(set.unlisted[0].name, name("rs:listing"));

    let lists = list_commands(&world);
    assert!(lists[listed..].iter().any(|args| {
        args.first().is_some_and(|arg| arg == "cargo")
            && args.iter().any(|arg| arg == "--workspace")
    }));
}
