//! Starting a command in a tree whose lease is still copying another tree's build.

use std::path::Path;
use std::sync::Arc;

use ignite_core::ids::TreeId;
use ignite_core::protocol::Class;
use ignite_core::traits::{CommandSpec, Launcher};

use super::SupervisedLauncher;

/// A check's listing compiled in a tree whose seed had not finished, so the seed found Cargo's
/// lock held when its copy was ready and threw the copy away. A command started anywhere in the
/// tree waits for the copy, says so, and starts once the copy is done.
#[test]
fn a_command_in_a_tree_being_seeded_waits_for_the_copy_and_says_so() {
    let dir = ignite_core::test_files::temp_dir()
        .join(format!("ignite-begin-seeding-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let tree = dir.join("tree-01");
    std::fs::create_dir_all(tree.join("rust")).unwrap();
    // No supervisor can start, so once the command stops waiting its launch fails at once.
    let launcher = Arc::new(SupervisedLauncher::new(
        &dir.join("no-such-ignite"),
        dir.join("runs"),
    ));
    let seeding = crate::trees::Seeding::start(tree.clone(), TreeId::new("tree-01").unwrap());
    let steps = crate::steps::Steps::new(Arc::new(ignite_core::time::SystemClock));
    let work = steps.begin(
        crate::steps::Kind::Verb("test".to_owned()),
        crate::steps::Work::default(),
    );
    let spec = CommandSpec {
        args: vec!["cargo".to_owned(), "nextest".to_owned(), "list".to_owned()],
        cwd: tree.join("rust"),
        env: Vec::new(),
        class: Class::Agent,
        cores: 1,
        resources: Vec::new(),
        grows: false,
        tree: None,
        ticket: None,
        hang_limit_s: None,
        reserve: None,
    };
    let runner = {
        let (launcher, work) = (Arc::clone(&launcher), Arc::clone(&work));
        std::thread::spawn(move || {
            let _entered = work.enter();
            launcher.run(&spec)
        })
    };
    while work.waits().is_empty() {
        assert!(!runner.is_finished(), "{:?}", runner.join());
        std::thread::yield_now();
    }
    assert_eq!(work.waits(), ["tree-01's build directories to be copied"]);
    assert!(!runs_started(&dir.join("runs")));

    drop(seeding);

    let result = runner.join().unwrap();
    assert_eq!(
        result.exit, None,
        "the launch went on and failed: {result:?}"
    );
    assert!(runs_started(&dir.join("runs")));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Whether any run directory was made, which a launch does before it starts the supervisor.
fn runs_started(runs: &Path) -> bool {
    std::fs::read_dir(runs).is_ok_and(|mut entries| {
        entries.any(|entry| entry.is_ok_and(|entry| entry.file_type().is_ok_and(|t| t.is_dir())))
    })
}
