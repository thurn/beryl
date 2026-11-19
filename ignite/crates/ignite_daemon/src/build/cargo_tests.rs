//! Cargo's package arguments and the feature comparison: a subset whose features differ builds
//! the whole workspace, and a product's `exclude` leaves packages out of it.

use super::*;

/// Every tip round's Rust batches failed with "enables different features than the whole
/// workspace", because a batch builds a few packages and the repository's workspace enables different
/// features for nearly every subset. Such a subset now builds the whole workspace instead.
#[test]
fn a_subset_that_changes_shared_features_builds_the_whole_workspace() {
    let f = fixture("features", false);
    f.launcher.with(|s| {
        s.subset_tree = "a v0.1.0 (/t/a)|\nserde v1.0.0|default,std (*)\nlibc v0.2.0|\n".into();
        s.whole_tree = "a v0.1.0 (/t/a)|\nserde v1.0.0|default,derive,std\nlibc v0.2.0|\n".into();
    });
    let result = f.manager.ensure(&tree(), "tests", &names(&["a"]));
    assert!(matches!(result, BuildResult::Built { .. }), "{result:?}");
    let builds = f.launcher.builds();
    assert_eq!(builds.len(), 1);
    assert!(builds[0].contains(&"--workspace".to_owned()), "{builds:?}");
    assert!(!builds[0].contains(&"-p".to_owned()), "{builds:?}");
    // The comparison is remembered until a Cargo file changes, and the whole build is current.
    f.manager.ensure(&tree(), "tests", &names(&["a"]));
    assert_eq!(f.launcher.tree_runs(), 2);
    assert_eq!(f.launcher.builds().len(), 1);
    f.git.set_object("Cargo.lock", oid('2'));
    f.manager.ensure(&tree(), "tests", &names(&["a"]));
    assert_eq!(f.launcher.tree_runs(), 4);
}

/// The Rust families built the workspace without one package while the test binaries' build
/// took the whole workspace, so Cargo compiled every crate twice with different features. A
/// product's `exclude` now leaves the package out of its whole build and of the comparison.
#[test]
fn a_whole_build_and_its_feature_tree_leave_out_excluded_packages() {
    let f = fixture("exclude", false);
    f.launcher
        .with(|s| s.subset_tree = "serde v1.0.0|std\n".into());
    f.manager.ensure(&tree(), "tests-less-b", &names(&["a"]));
    let less_b = names(&["cargo", "test", "--no-run", "--workspace", "--exclude", "b"]);
    assert_eq!(f.launcher.builds(), std::slice::from_ref(&less_b));
    let whole_tree = f
        .launcher
        .with(|s| s.runs.iter().any(|r| r.args.ends_with(&less_b[3..])));
    assert!(whole_tree, "the whole feature tree did not leave out b");
}

/// A command without `{packages}` builds the same thing for any packages, so it compares no
/// feature trees and is one request whichever packages its callers name.
#[test]
fn a_command_without_packages_compares_no_features() {
    let f = fixture("fixed", false);
    f.manager.ensure(&tree(), "fixed", &names(&["a"]));
    let second = f.manager.ensure(&tree(), "fixed", &names(&["b"]));
    assert_eq!((second, f.launcher.tree_runs()), (BuildResult::Current, 0));
}

#[test]
fn feature_differences_ignore_feature_nodes_and_repeats() {
    let whole = "x v1|a,b\nx feature \"a\"\nx v1|a,b (*)\ny v2|\n";
    assert!(feature_differences(whole, "x v1|a,b\ny v2|\n").is_empty());
    assert_eq!(feature_differences(whole, "x v1|a\n"), ["x"]);
}
