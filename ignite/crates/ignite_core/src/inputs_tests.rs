use std::path::{Path, PathBuf};

use super::*;
use crate::fakes::FakeGit;
use crate::hash::{HashParts, input_hash};

/// `cargo metadata` output for a workspace at `/repo/ws`: `app` depends on `core` normally and
/// on `testkit` only for its tests; `core` depends on `base` and on a registry crate.
const METADATA: &str = r#"{
  "packages": [
    {"name": "app", "manifest_path": "/repo/ws/crates/app/Cargo.toml", "dependencies": [
      {"name": "core", "kind": null, "path": "/repo/ws/crates/core"},
      {"name": "testkit", "kind": "dev", "path": "/repo/ws/crates/testkit"}
    ]},
    {"name": "core", "manifest_path": "/repo/ws/crates/core/Cargo.toml", "dependencies": [
      {"name": "base", "kind": null, "path": "/repo/ws/crates/base"},
      {"name": "serde", "kind": null, "req": "^1"}
    ]},
    {"name": "base", "manifest_path": "/repo/ws/crates/base/Cargo.toml", "dependencies": []},
    {"name": "testkit", "manifest_path": "/repo/ws/crates/testkit/Cargo.toml",
     "dependencies": []},
    {"name": "lonely", "manifest_path": "/repo/ws/crates/lonely/Cargo.toml", "dependencies": []}
  ],
  "workspace_root": "/repo/ws",
  "workspace_members": []
}"#;

fn graph() -> CrateGraph {
    let mut graph = CrateGraph::default();
    graph.add_workspace(METADATA, "/repo").unwrap();
    graph
}

fn tracked() -> Vec<String> {
    [
        "rust-toolchain.toml",
        "ws/Cargo.lock",
        "ws/Cargo.toml",
        "ws/crates/app/src/lib.rs",
        "ws/crates/base/src/lib.rs",
        "ws/crates/core/src/lib.rs",
        "ws/crates/lonely/src/lib.rs",
        "ws/crates/testkit/src/lib.rs",
        "web/src/main.ts",
        "web/src/stage/phone.ts",
        "web/shots.json",
        "tools/tests/test_one.py",
        "tools/tests/test_two.py",
        "tools/tests/helper.py",
        "README.md",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect()
}

fn patterns(list: &[&str]) -> Vec<InputPattern> {
    list.iter()
        .map(|p| InputPattern::from((*p).to_owned()))
        .collect()
}

#[test]
fn metadata_gives_relative_dirs_and_workspace_dependencies() {
    let graph = graph();
    let app = graph.package("app").unwrap();
    assert_eq!(app.dir, "ws/crates/app");
    assert_eq!(app.workspace, "ws");
    assert_eq!(app.deps, ["core", "testkit"]);
    assert_eq!(graph.package("core").unwrap().deps, ["base"]);
}

#[test]
fn a_closure_includes_development_dependencies() {
    assert_eq!(
        closure("app", &graph()).unwrap(),
        ["app", "base", "core", "testkit"]
    );
    assert!(closure("missing", &graph()).is_err());
}

#[test]
fn crate_closure_adds_package_dirs_the_lock_file_and_the_toolchain() {
    let resolved = resolve(
        &patterns(&["crate-closure"]),
        Some("core"),
        &graph(),
        &tracked(),
    );
    assert_eq!(
        resolved.unwrap(),
        [
            "rust-toolchain.toml",
            "ws/Cargo.lock",
            "ws/crates/base",
            "ws/crates/core"
        ]
    );
    let error = resolve(&patterns(&["crate-closure"]), None, &graph(), &tracked()).unwrap_err();
    assert_eq!(error.pattern, "crate-closure");
}

#[test]
fn path_patterns_name_directories_files_and_globs() {
    let resolved = resolve(
        &patterns(&["web/src/**", "web/shots.json", "tools/tests/test_*.py"]),
        None,
        &graph(),
        &tracked(),
    )
    .unwrap();
    assert_eq!(
        resolved,
        [
            "tools/tests/test_one.py",
            "tools/tests/test_two.py",
            "web/shots.json",
            "web/src"
        ]
    );
    let deep = resolve(&patterns(&["**/phone.ts"]), None, &graph(), &tracked()).unwrap();
    assert_eq!(deep, ["web/src/stage/phone.ts"]);
}

#[test]
fn paths_inside_a_named_directory_are_dropped() {
    let resolved = resolve(
        &patterns(&["web/src/main.ts", "web/**", "crate:base", "ws/crates/**"]),
        None,
        &graph(),
        &tracked(),
    )
    .unwrap();
    assert_eq!(
        resolved,
        ["rust-toolchain.toml", "web", "ws/Cargo.lock", "ws/crates"]
    );
}

#[test]
fn a_pattern_that_matches_nothing_is_an_error() {
    for pattern in [
        "web/missing.ts",
        "web/nothing/**",
        "web/*.rs",
        "crate:ghost",
    ] {
        let error = resolve(&patterns(&[pattern]), None, &graph(), &tracked()).unwrap_err();
        assert!(
            error.pattern.contains(pattern.trim_start_matches("crate:")),
            "{error}"
        );
    }
}

#[test]
fn single_segment_wildcards_stay_within_a_segment() {
    assert!(segment_matches("test_*.py", "test_one.py"));
    assert!(segment_matches("*", ""));
    assert!(segment_matches("a*b*c", "aXbYbZc"));
    assert!(segment_matches("?x", "ax"));
    assert!(!segment_matches("test_*.py", "test_one.pyc"));
    assert!(!glob(&["*.py"], &["tools", "a.py"]));
    assert!(glob(&["**", "*.py"], &["tools", "a.py"]));
    assert!(glob(&["tools", "**"], &["tools"]));
}

#[test]
fn the_hash_moves_with_its_inputs_and_not_with_other_files() {
    let git = FakeGit::default();
    for (i, path) in tracked().iter().enumerate() {
        git.set_object(path, Oid::new(format!("{i:040x}")).unwrap());
    }
    for (i, dir) in ["web/src", "ws/crates/base", "ws/crates/core"]
        .iter()
        .enumerate()
    {
        git.set_object(dir, Oid::new(format!("{:040x}", 100 + i)).unwrap());
    }
    let hash_of = |git: &FakeGit| {
        let paths = resolve(
            &patterns(&["crate:core", "web/src/**"]),
            None,
            &graph(),
            &tracked(),
        )
        .unwrap();
        let ids = object_ids(git, Path::new("tree"), &paths).unwrap();
        input_hash(&HashParts {
            inputs: &ids,
            commands: &[],
            toolchain: "1.93.0",
            platform: None,
        })
    };
    let before = hash_of(&git);
    git.set_object("README.md", Oid::new("f".repeat(40)).unwrap());
    git.set_object(
        "ws/crates/lonely/src/lib.rs",
        Oid::new("e".repeat(40)).unwrap(),
    );
    assert_eq!(hash_of(&git), before, "an unrelated file moved the hash");
    git.set_object("ws/crates/base", Oid::new("d".repeat(40)).unwrap());
    assert_ne!(
        hash_of(&git),
        before,
        "a dependency's tree did not move the hash"
    );
}

#[test]
fn areas_are_packages_or_second_level_directories() {
    let graph = graph();
    assert_eq!(
        area("ws/crates/core/src/lib.rs", &graph),
        Area::Package("core".to_owned())
    );
    assert_eq!(
        area("web/src/stage/phone.ts", &graph),
        Area::Directory("web/src".to_owned())
    );
    assert_eq!(area("ws/Cargo.toml", &graph).to_string(), "ws/");
    assert_eq!(area("README.md", &graph).to_string(), "README.md/");
}

#[test]
fn lint_names_each_pattern_that_matches_nothing() {
    let text = r#"
[family.f]
kind = "regression"
adapter = "nextest"
platforms = "any"
needs = ["p"]
inputs = ["crate-closure", "web/src/**", "web/gone/**", "crate:ghost"]

[product.p]
build = "cargo build"
inputs = ["ws/Cargo.lock", "ws/*.json"]
outputs = ["ws/target"]
"#;
    let manifest = Manifest::from_files(&[(PathBuf::from("f.toml"), text.to_owned())]).unwrap();
    let findings: Vec<String> = lint(&manifest, &graph(), &tracked())
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        findings,
        [
            "family.f.inputs[2]: web/gone/**: matches no tracked file",
            "family.f.inputs[3]: crate:ghost: names no workspace package",
            "product.p.inputs[1]: ws/*.json: matches no tracked file",
        ]
    );
}
