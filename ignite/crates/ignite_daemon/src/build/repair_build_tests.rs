//! A build whose failure comes from damage to the tree's build products is cleared and run once
//! more (see `build::repair`).

use super::*;
use crate::build::repair::SUSPECT_FILE;

/// What Cargo printed when the test target `session` failed to link in a landing tree whose
/// incremental build was damaged, shortened.
const LINK_FAILURE: &str = "\
error: linking with `cc` failed: exit status: 1
  = note: Undefined symbols for architecture arm64:
            \"__RINvNtCs8Mbv00yxnRz_4core3ptr9drop_glue…7session.llvm.6201283016186011144\", referenced from:
          ld: symbol(s) not found for architecture arm64

error: could not compile `game_session` (test \"session\") due to 1 previous error
";

/// A plain source error, which the same inputs repeat wherever they build.
const TYPE_ERROR: &str = "\
error[E0308]: mismatched types
error: could not compile `b` (lib) due to 1 previous error
";

/// Files of the crate `session` and of a neighbour in the target directory of `f`'s tree.
fn damaged_target(f: &Fixture) -> PathBuf {
    let debug = f.dir.join("target").join("debug");
    let session = debug.join("incremental").join("session-1imqhd34qdwcf");
    fs::create_dir_all(session.join("s-hoa6uqen2b-0h7twqo-4x2xk8x6ccst6d0q82uj5cysn")).unwrap();
    fs::create_dir_all(
        debug
            .join("incremental")
            .join("session_types-3nuzzneutc6f3"),
    )
    .unwrap();
    let deps = debug.join("deps");
    fs::create_dir_all(&deps).unwrap();
    for file in [
        "session-1d2e56b5d8df6699",
        "session-1d2e56b5d8df6699.d",
        "session-1d2e56b5d8df6699.7gfw9a8kpzcbtgz8agabli17j.0q9if7e.rcgu.o",
        "session_types-0123456789abcdef",
        "libgame_session-7be26d0c1855ef7e.rlib",
    ] {
        fs::write(deps.join(file), "x").unwrap();
    }
    debug
}

fn present(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[test]
fn a_link_failure_clears_the_crate_and_builds_once_more() {
    let f = fixture("repair-link", false);
    let debug = damaged_target(&f);
    f.launcher
        .with(|s| s.fail_with.push_back(LINK_FAILURE.to_owned()));

    let result = f.manager.ensure(&tree(), "tests", &names(&["b"]));
    let BuildResult::Built { usage, .. } = &result else {
        panic!("the second build passes: {result:?}");
    };
    assert_eq!(f.launcher.builds().len(), 2);
    assert!((usage.wall_s - 6.0).abs() < 1e-9, "both builds count");
    // Only the crate that failed lost its incremental work and outputs.
    assert_eq!(
        present(&debug.join("incremental")),
        ["session_types-3nuzzneutc6f3"]
    );
    assert_eq!(
        present(&debug.join("deps")),
        [
            "libgame_session-7be26d0c1855ef7e.rlib",
            "session_types-0123456789abcdef"
        ]
    );
    assert_eq!(
        f.manager.ensure(&tree(), "tests", &names(&["b"])),
        BuildResult::Current
    );
}

/// What Cargo printed when the binary `record` failed to link because its dependency's library,
/// not `record`, held objects from two compiles, shortened.
const DEPENDENCY_LINK_FAILURE: &str = "\
error: linking with `cc` failed: exit status: 1
  = note: Undefined symbols for architecture arm64:
            \"_anon.2e5a857eea634338498684503b2aa78f.5.llvm.12721422738467408669\", referenced from:
                __RINvXs4_serde_json in libsession_types-0123456789abcdef.rlib[43](session_types-0123456789abcdef.41y.04s541b.rcgu.o)
          ld: symbol(s) not found for architecture arm64

error: could not compile `game_record` (bin \"record\") due to 1 previous error
";

/// A link error in a dependency's library clears that library, not only the binary that failed.
#[test]
fn a_link_failure_in_a_dependency_clears_the_dependency() {
    let f = fixture("repair-dependency", false);
    let debug = damaged_target(&f);
    f.launcher
        .with(|s| s.fail_with.push_back(DEPENDENCY_LINK_FAILURE.to_owned()));
    let result = f.manager.ensure(&tree(), "tests", &names(&["b"]));
    assert!(matches!(result, BuildResult::Built { .. }), "{result:?}");
    assert_eq!(f.launcher.builds().len(), 2);
    assert_eq!(
        present(&debug.join("incremental")),
        ["session-1imqhd34qdwcf"]
    );
    let deps = present(&debug.join("deps"));
    assert!(
        deps.iter().all(|name| !name.starts_with("session_types")),
        "{deps:?}"
    );
    assert!(deps.contains(&"session-1d2e56b5d8df6699".to_owned()));
}

#[test]
fn a_build_that_fails_twice_is_reported_and_remembered() {
    let f = fixture("repair-twice", false);
    damaged_target(&f);
    f.launcher.with(|s| {
        s.fail_with.push_back(LINK_FAILURE.to_owned());
        s.fail_with.push_back(LINK_FAILURE.to_owned());
        s.fail_with.push_back(LINK_FAILURE.to_owned());
    });
    let first = f.manager.ensure(&tree(), "tests", &names(&["b"]));
    assert!(matches!(first, BuildResult::Failed { .. }), "{first:?}");
    assert_eq!(f.launcher.builds().len(), 2, "one repair, not more");
    let again = f.manager.ensure(&tree(), "tests", &names(&["b"]));
    assert!(matches!(again, BuildResult::Failed { .. }), "{again:?}");
    assert_eq!(f.launcher.builds().len(), 2);
}

#[test]
fn a_source_error_is_not_built_again() {
    let f = fixture("repair-source", false);
    let debug = damaged_target(&f);
    f.launcher
        .with(|s| s.fail_with.push_back(TYPE_ERROR.to_owned()));
    let result = f.manager.ensure(&tree(), "tests", &names(&["b"]));
    assert!(matches!(result, BuildResult::Failed { .. }), "{result:?}");
    assert_eq!(f.launcher.builds().len(), 1);
    assert_eq!(present(&debug.join("incremental")).len(), 2);
}

/// Any failure of inputs that another tree built is this tree's own, so it is repaired.
#[test]
fn a_failure_of_inputs_another_tree_built_clears_the_crate() {
    let f = fixture("repair-elsewhere", false);
    let plan = f.manager.plan(&tree(), "tests", &names(&["b"])).unwrap();
    let other = Request {
        tree: TreeId::new("tree-02").unwrap(),
        ..plan.request.clone()
    };
    lock(&f.manager.state).held.insert(other, plan.hash.clone());
    f.launcher
        .with(|s| s.fail_with.push_back(TYPE_ERROR.to_owned()));
    let result = f.manager.ensure(&tree(), "tests", &names(&["b"]));
    assert!(matches!(result, BuildResult::Built { .. }), "{result:?}");
    assert_eq!(f.launcher.builds().len(), 2);
}

/// A build ended partway marks its target directory, and the next build first clears each
/// unit whose compile it left unfinished, and no other build of that crate.
#[test]
fn a_build_ended_partway_leaves_the_next_build_to_clear_its_unfinished_units() {
    let f = fixture("repair-ended", false);
    let debug = damaged_target(&f);
    f.launcher.with(|s| s.failing = Some((None, false)));
    let ended = f.manager.ensure(&tree(), "tests", &names(&["b"]));
    assert!(matches!(ended, BuildResult::Failed { .. }), "{ended:?}");
    let target = f.dir.join("target");
    assert!(target.join(SUSPECT_FILE).is_file());

    // rustc names a session folder `-working` until its compile finishes.
    let working = debug
        .join("incremental")
        .join("session_types-3nuzzneutc6f3")
        .join("s-hoa6v2nlqi-0qrw2mj-working");
    fs::create_dir_all(&working).unwrap();
    // The session's codegen unit is one of the unit `session_types-0123456789abcdef`, which has
    // a fingerprint folder; another build of the crate has nothing unfinished.
    fs::write(working.join("1cgu.o"), "x").unwrap();
    let deps = debug.join("deps");
    fs::write(
        deps.join("session_types-0123456789abcdef.1cgu.0q9if7e.rcgu.o"),
        "x",
    )
    .unwrap();
    fs::write(deps.join("session_types-fedcba9876543210"), "x").unwrap();
    let fingerprint = debug
        .join(".fingerprint")
        .join("session_types-0123456789abcdef");
    fs::create_dir_all(&fingerprint).unwrap();
    f.launcher.with(|s| s.failing = None);
    let built = f.manager.ensure(&tree(), "tests", &names(&["b"]));
    assert!(matches!(built, BuildResult::Built { .. }), "{built:?}");
    assert!(!target.join(SUSPECT_FILE).exists());
    assert_eq!(
        present(&debug.join("incremental")),
        ["session-1imqhd34qdwcf"]
    );
    let deps = present(&deps);
    assert!(
        deps.iter().all(|name| !name.contains("0123456789abcdef")),
        "{deps:?}"
    );
    assert!(deps.contains(&"session_types-fedcba9876543210".to_owned()));
    assert!(!fingerprint.exists());
}
