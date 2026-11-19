use ignite_core::fakes::FakeGit;

use super::*;

#[test]
fn the_client_tests_and_markdown_do_not_affect_the_daemon() {
    for path in [
        "ignite/crates/ignite_cli/src/reattach.rs",
        "ignite/crates/ignite_cli/Cargo.toml",
        "ignite/crates/ignite_daemon/src/trees/disk/tests.rs",
        "ignite/crates/ignite_daemon/src/dispatchd_models_tests.rs",
        "ignite/crates/ignite_daemon/src/land/tests/fakes.rs",
        "ignite/crates/ignite_hub/tests/dashboard_components.rs",
        "ignite/AGENTS.md",
        "docs/ignite.md",
        "ignite.toml",
    ] {
        assert!(!affects_daemon("ignite", path), "{path}");
    }
}

#[test]
fn the_daemons_crates_their_manifests_and_embedded_files_affect_it() {
    for path in [
        "ignite/crates/ignite_daemon/src/trees/disk.rs",
        "ignite/crates/ignite_core/src/config.rs",
        "ignite/crates/ignite_hub/src/routes/dispatch_queue.rs",
        "ignite/crates/ignite_hub/static/cirrus.js",
        "ignite/crates/ignite_core/src/guide/release.txt",
        "ignite/crates/ignite_daemon/src/testhub.rs",
        "ignite/Cargo.lock",
        "ignite/crates/ignite_os/Cargo.toml",
    ] {
        assert!(affects_daemon("ignite", path), "{path}");
    }
}

#[test]
fn daemon_changes_keep_only_the_daemons_files_and_summarise_them() {
    let git = FakeGit::default();
    let (from, to) = (
        Commit::new("a1b2c3d").unwrap(),
        Commit::new("e4f5a6b").unwrap(),
    );
    git.set_changed(
        &from,
        &to,
        &[
            "ignite/crates/ignite_cli/src/main.rs",
            "ignite/crates/ignite_daemon/src/update.rs",
            "ignite/crates/ignite_daemon/src/update/tests.rs",
            "ignite/crates/ignite_core/src/config.rs",
        ],
    );
    let changes = daemon_changes(&git, "ignite", &from, &to).unwrap();
    assert_eq!(
        changes,
        [
            "ignite/crates/ignite_core/src/config.rs",
            "ignite/crates/ignite_daemon/src/update.rs",
        ]
    );
    assert_eq!(
        summary(&["a".into(), "b".into(), "c".into(), "d".into(), "e".into()]),
        "a, b, c and 2 more"
    );
}
