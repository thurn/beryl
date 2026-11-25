//! A machine's `tip_families`: the families it lists, builds and runs in tip rounds. A family
//! left out is keyed from the listing another machine kept at the hub, and nothing is built
//! for it here.

use super::*;

use crate::tip::ListingPhase;

fn tree() -> TreeId {
    TreeId::new("tree-01").unwrap()
}

/// A slow Windows machine that verified every family in tip rounds built the products of
/// portable families whose checks the Macs had already claimed. With `tip_families` naming
/// only its Windows family, it lists and builds nothing for `rust`, and still keys `rust`'s
/// checks from the Mac's kept listing so that the round knows what must pass.
#[test]
fn a_family_left_out_of_tip_families_is_never_listed_or_built_in_a_tip_round() {
    let manifest = r#"
[product.binary]
build = "builder"
inputs = ["src"]
outputs = ["out"]

[family.rust]
kind = "regression"
adapter = "jsonl"
platforms = "any"
needs = ["binary"]
builds = true
list = "rust-lister"
run = "runner {checks}"
inputs = ["src"]

[family.win]
kind = "regression"
adapter = "jsonl"
platforms = ["windows"]
list = "win-lister"
run = "runner {checks}"
inputs = ["src"]
"#;
    let world = world(manifest, &[], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() =
        "{\"check\": \"rust:a\"}\n{\"check\": \"win:w\"}\n".to_owned();
    let mut windows = daemon(
        &world,
        ignite_store::local::open_in_memory().unwrap(),
        FakeTrees::new(&world.dir, &[]),
    );
    let settings = &mut Arc::get_mut(&mut windows).unwrap().settings;
    settings.os = Os::Windows;
    settings.tip_families = Some(vec!["win".to_owned()]);

    // Before any machine has listed `rust`, the round waits for its listing.
    let cold = windows.regression_checks(&world.ctx, &tree()).unwrap();
    assert!(world.builds.requests().is_empty(), "nothing was built");
    assert!(world.launcher.commands("rust-lister").is_empty());
    assert_eq!(world.launcher.commands("win-lister").len(), 1);
    let checks: Vec<(&str, bool)> = cold
        .checks
        .iter()
        .map(|c| (c.check.as_str(), c.runnable))
        .collect();
    assert_eq!(checks, [("win:w", true)]);
    assert_eq!(cold.unlisted.len(), 1, "{:?}", cold.unlisted);
    assert_eq!(cold.unlisted[0].name, CheckName::new("rust:listing").unwrap());
    assert!(!cold.unlisted[0].reportable);

    // The Mac lists `rust`, building its product, and keeps the listing at the hub.
    let mac = world.checks.regression_checks(&world.ctx, &tree()).unwrap();
    let mac_rust = mac.checks.iter().find(|c| c.family == "rust").unwrap();
    let builds = world.builds.requests().len();
    assert!(builds > 0);
    let rust_lists = world.launcher.commands("rust-lister").len();

    let build_phase = windows
        .regression_checks_for(&world.ctx, &tree(), ListingPhase::Build)
        .unwrap();
    assert!(build_phase.checks.is_empty(), "{:?}", build_phase.checks);
    assert!(build_phase.unlisted.is_empty(), "{:?}", build_phase.unlisted);
    let warm = windows.regression_checks(&world.ctx, &tree()).unwrap();
    assert!(warm.unlisted.is_empty(), "{:?}", warm.unlisted);
    let rust = warm.checks.iter().find(|c| c.family == "rust").unwrap();
    assert_eq!(rust.key, mac_rust.key);
    assert!(!rust.runnable);
    assert_eq!(world.builds.requests().len(), builds, "Windows built nothing");
    assert_eq!(world.launcher.commands("rust-lister").len(), rust_lists);
}
