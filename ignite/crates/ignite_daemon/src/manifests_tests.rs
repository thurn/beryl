use std::path::{Path, PathBuf};
use std::sync::Arc;

use ignite_core::fakes::{FakeBuilds, FakeClock};
use ignite_core::time::Timestamp;
use ignite_store::local::open_in_memory;

use super::*;
use crate::trees::TreeConfig;

const CONFIG: &str = r#"
[repository]
remote = "origin"
staging = "staging"
master = "master"
trees = "trees"
checks = "tools/checks"
[hub]
url = "http://localhost:1"
project = "test"
[gate]
threshold = 0.004
cap = 400
[machine.local]
ticket_trees = 1
landing_trees = 1
"#;

/// A product named `name` that writes `out` and whose inputs name a workspace package, so it is
/// also prebuilt.
fn product(name: &str, out: &str) -> String {
    format!(
        "[product.{name}]\nbuild = \"make {name}\"\ninputs = [\"crate:{name}\"]\noutputs = \
         [\"{out}\"]\n"
    )
}

fn write_manifest(repo: &Path, text: &str) {
    let dir = repo.join("tools/checks");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("products.toml"), text).unwrap();
}

/// A reload over a main checkout at `repo`, holding the manifests found there now.
fn reload_at(repo: &Path) -> (Reload, TreeConfig) {
    let config = Config::parse("ignite.toml", CONFIG).unwrap();
    let manifest = crate::main::read_manifest(repo, &config).unwrap();
    let tree_config = TreeConfig::new(repo, &config, "local", &manifest).unwrap();
    let clock = Arc::new(FakeClock::new(Timestamp::EPOCH));
    let trees = Trees::new(
        tree_config.clone(),
        open_in_memory().unwrap(),
        clock,
        Arc::new(FakeBuilds::default()),
    );
    let reload = Reload {
        repo: repo.to_path_buf(),
        config,
        manifest: manifest.into(),
        trees,
    };
    (reload, tree_config)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ignite-manifests-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A product added to the main checkout's manifests reaches the build manager's and the idle
/// work's manifests and the trees' keep and prebuild lists without a restart; reading the same
/// manifests again changes nothing.
#[test]
fn a_product_added_to_the_main_checkout_is_read_without_a_restart() {
    let repo = scratch("added");
    write_manifest(&repo, &product("first", "first/target"));
    let (reload, tree_config) = reload_at(&repo);
    let held_by_builds = reload.manifest.clone();
    assert_eq!(*tree_config.keep.get(), ["first/target"]);

    write_manifest(
        &repo,
        &format!(
            "{}\n{}",
            product("first", "first/target"),
            product("second", "second/out")
        ),
    );
    assert_eq!(reload.run(), Ok(true));

    assert!(held_by_builds.get().products.contains_key("second"));
    assert_eq!(*tree_config.keep.get(), ["first/target", "second/out"]);
    assert_eq!(*tree_config.prebuild.get(), ["first", "second"]);
    assert_eq!(reload.run(), Ok(false));
    let _ = std::fs::remove_dir_all(&repo);
}

/// Manifests that cannot be parsed, such as a checkout halfway through a merge, leave the held
/// ones in place.
#[test]
fn manifests_that_do_not_parse_leave_the_held_ones() {
    let repo = scratch("broken");
    write_manifest(&repo, &product("first", "first/target"));
    let (reload, tree_config) = reload_at(&repo);

    write_manifest(&repo, "[product.first\n");
    assert!(reload.run().is_err());

    assert!(reload.manifest.get().products.contains_key("first"));
    assert_eq!(*tree_config.keep.get(), ["first/target"]);
    let _ = std::fs::remove_dir_all(&repo);
}
