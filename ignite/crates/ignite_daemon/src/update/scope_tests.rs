use ignite_core::fakes::FakeGit;

use super::*;

/// Ignite's source and check manifests as this kind of repository lays them out: the manifests
/// inside Ignite's source directory.
const AT: Source<'static> = Source {
    source: "ignite",
    checks: "ignite/checks",
};

#[test]
fn the_client_tests_markdown_and_guide_text_do_not_affect_the_daemon() {
    for path in [
        "ignite/crates/ignite_cli/src/reattach.rs",
        "ignite/crates/ignite_cli/Cargo.toml",
        "ignite/crates/ignite_daemon/src/trees/disk/tests.rs",
        "ignite/crates/ignite_daemon/src/dispatchd_models_tests.rs",
        "ignite/crates/ignite_daemon/src/land/tests/fakes.rs",
        "ignite/crates/ignite_hub/tests/dashboard_components.rs",
        "ignite/crates/ignite_core/src/guide/justiciar.txt",
        "ignite/AGENTS.md",
        "docs/ignite.md",
        "ignite.toml",
    ] {
        assert!(!affects_daemon(AT, path), "{path}");
    }
}

#[test]
fn the_daemons_crates_their_manifests_and_embedded_files_affect_it() {
    for path in [
        "ignite/crates/ignite_daemon/src/trees/disk.rs",
        "ignite/crates/ignite_core/src/config.rs",
        "ignite/crates/ignite_hub/src/routes/dispatch_queue.rs",
        "ignite/crates/ignite_hub/static/cirrus.js",
        "ignite/crates/ignite_core/src/guide/mod.rs",
        "ignite/crates/ignite_core/src/protocol/client.txt",
        "ignite/crates/ignite_daemon/src/testhub.rs",
        "ignite/Cargo.lock",
        "ignite/crates/ignite_os/Cargo.toml",
    ] {
        assert!(affects_daemon(AT, path), "{path}");
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
            "ignite/checks/rest.toml",
        ],
    );
    let changes = daemon_changes(&git, AT, &from, &to).unwrap();
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

#[test]
fn only_the_land_module_outside_its_tests_affects_a_landing() {
    for path in [
        "ignite/crates/ignite_daemon/src/land.rs",
        "ignite/crates/ignite_daemon/src/land/express.rs",
    ] {
        assert!(affects_landing(AT, path), "{path}");
    }
    for path in [
        "ignite/crates/ignite_daemon/src/land/express_tests.rs",
        "ignite/crates/ignite_daemon/src/land/tests/fakes.rs",
        "ignite/crates/ignite_daemon/src/landing.rs",
        "ignite/crates/ignite_daemon/src/main/selfupdate.rs",
        "ignite/crates/ignite_core/src/config.rs",
    ] {
        assert!(!affects_landing(AT, path), "{path}");
    }
    let git = FakeGit::default();
    let (from, to) = (
        Commit::new("a1b2c3d").unwrap(),
        Commit::new("e4f5a6b").unwrap(),
    );
    git.set_changed(
        &from,
        &to,
        &[
            "ignite/crates/ignite_daemon/src/main/loops.rs",
            "ignite/crates/ignite_daemon/src/land/lease.rs",
        ],
    );
    assert_eq!(
        landing_changes(&git, AT, &from, &to).unwrap(),
        ["ignite/crates/ignite_daemon/src/land/lease.rs"]
    );
}

#[test]
fn only_the_tip_module_outside_its_tests_affects_a_tip_round() {
    for path in [
        "ignite/crates/ignite_daemon/src/tip.rs",
        "ignite/crates/ignite_daemon/src/tip/words.rs",
    ] {
        assert!(affects_tip(AT, path), "{path}");
    }
    for path in [
        "ignite/crates/ignite_daemon/src/tip/words_tests.rs",
        "ignite/crates/ignite_daemon/src/tip/tests/fakes.rs",
        "ignite/crates/ignite_daemon/src/land/lease.rs",
        "ignite/crates/ignite_daemon/src/main/selfupdate.rs",
        "ignite/crates/ignite_core/src/config.rs",
    ] {
        assert!(!affects_tip(AT, path), "{path}");
    }
    let git = FakeGit::default();
    let (from, to) = (
        Commit::new("a1b2c3d").unwrap(),
        Commit::new("e4f5a6b").unwrap(),
    );
    git.set_changed(
        &from,
        &to,
        &[
            "ignite/crates/ignite_daemon/src/main/loops.rs",
            "ignite/crates/ignite_daemon/src/tip/words.rs",
        ],
    );
    assert_eq!(
        tip_changes(&git, AT, &from, &to).unwrap(),
        ["ignite/crates/ignite_daemon/src/tip/words.rs"]
    );
}

/// The check manifests lie under Ignite's source directory but are read from the checkout at run
/// time, so a change to them is neither a change to Ignite's source nor to the daemon's code.
#[test]
fn the_check_manifests_are_not_ignites_source() {
    for path in ["ignite/checks/native.toml", "ignite/checks/rest.toml"] {
        assert!(!ignite_change(AT, path), "{path}");
        assert!(!affects_daemon(AT, path), "{path}");
    }
    // Only the directory itself is excluded, not a sibling whose name starts the same way.
    assert!(ignite_change(AT, "ignite/checks_list.rs"));
    assert!(ignite_change(
        AT,
        "ignite/crates/ignite_core/src/manifest.rs"
    ));
    assert!(!ignite_change(AT, "docs/ignite.md"));
}

/// The manifests' directory is taken as configured, with or without a trailing slash, and
/// manifests outside Ignite's source change nothing either way.
#[test]
fn the_manifest_directory_is_whatever_the_configuration_names() {
    let trailing = Source {
        source: "ignite/",
        checks: "ignite/checks/",
    };
    assert!(!ignite_change(trailing, "ignite/checks/a.toml"));
    assert!(ignite_change(trailing, "ignite/Cargo.toml"));
    let apart = Source {
        source: "ignite",
        checks: "checks",
    };
    assert!(!ignite_change(apart, "checks/a.toml"));
    assert!(ignite_change(apart, "ignite/checks/a.toml"));
    assert!(affects_daemon(apart, "ignite/checks/a.toml"));
}
