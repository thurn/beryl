//! Landings whose formatters change files, and landings with nothing to format.

use std::path::Path;
use std::sync::{Arc, Mutex};

use ignite_core::protocol::client::Exit;

use super::fakes::{TestGit, edits};
use super::{World, world};
use crate::land::Landing;
use crate::land::format::{Formatted, Formatter, SUBJECT};
use crate::land::records::Records;

/// Formatters that rewrite `fixes` when the landing changes any file, committing through the
/// fake git, and record the files they were given.
struct FakeFormat {
    git: Arc<TestGit>,
    fixes: Vec<(&'static str, &'static str)>,
    given: Mutex<Vec<Vec<String>>>,
}

impl Formatter for FakeFormat {
    fn format(&self, tree: &Path, changed: &[String]) -> Formatted {
        self.given.lock().unwrap().push(changed.to_vec());
        Formatted {
            ran: vec!["rustfmt {files}".to_owned(), "mdformat {files}".to_owned()],
            failed: Vec::new(),
            commit: (!self.fixes.is_empty()).then(|| self.git.commit_on(tree, &self.fixes)),
        }
    }
}

/// The default world with the formatters `fixes` on the machine.
fn format_world(fixes: Vec<(&'static str, &'static str)>) -> (World, Arc<FakeFormat>) {
    let mut world = world();
    let format = Arc::new(FakeFormat {
        git: world.git.clone(),
        fixes,
        given: Mutex::new(Vec::new()),
    });
    let pause_clock = world.clock.clone();
    world.landing = Arc::new(Landing {
        trees: world.trees.clone(),
        gate: world.gate.clone(),
        records: Records::new(&world.records),
        express: None,
        format: Some(format.clone()),
        pause: Arc::new(move |seconds| pause_clock.advance(seconds)),
    });
    (world, format)
}

#[test]
fn a_landing_with_misformatted_rust_and_markdown_files_lands_with_a_format_commit() {
    let (world, format) = format_world(vec![("a.rs", "fn a() {}"), ("doc.md", "* item")]);
    world.hold(
        "tree-01",
        "t.1",
        "local_a",
        &[("a.rs", "fn  a() {}"), ("doc.md", "*  item")],
    );

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(
        reply.text.contains("2 formatters ran: committed"),
        "{}",
        reply.text
    );
    assert!(reply.text.contains(SUBJECT), "{}", reply.text);
    assert_eq!(
        *format.given.lock().unwrap(),
        [vec!["a.rs".to_owned(), "doc.md".to_owned()]]
    );
    // What landed is the formatted text of both files.
    let pushes = world.git.pushes();
    assert_eq!(pushes.len(), 1);
    let repo = world.git.repo.lock().unwrap();
    let landed = &repo.files[&pushes[0].1];
    let formatted = edits(&[("a.rs", "fn a() {}"), ("doc.md", "* item")]);
    assert_eq!(landed["a.rs"], formatted["a.rs"]);
    assert_eq!(landed["doc.md"], formatted["doc.md"]);
}

#[test]
fn a_landing_with_nothing_to_format_adds_no_commit() {
    let (world, format) = format_world(Vec::new());
    world.hold("tree-01", "t.1", "local_a", &[("a.rs", "fn a() {}")]);
    let head =
        world.git.repo.lock().unwrap().checked_out[Path::new("/ignite-test-trees/tree-01")].clone();

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(
        reply.text.contains("2 formatters ran: nothing to change"),
        "{}",
        reply.text
    );
    assert_eq!(format.given.lock().unwrap().len(), 1);
    assert_eq!(
        world.git.pushes()[0].1,
        head,
        "the branch's own commit landed"
    );
}

#[test]
fn the_formatters_run_once_however_many_passes_the_landing_takes() {
    let (world, format) = format_world(vec![("a.rs", "fn a() {}")]);
    world.hold("tree-01", "t.1", "local_a", &[("a.rs", "fn  a() {}")]);
    // Another machine lands just before the first push, so the landing takes a second pass.
    world
        .git
        .landed_before_push
        .lock()
        .unwrap()
        .push_back(vec![("c.txt", "c by another")]);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(reply.text.contains("in pass 2 ("), "{}", reply.text);
    assert_eq!(format.given.lock().unwrap().len(), 1);
    let pushes = world.git.pushes();
    let repo = world.git.repo.lock().unwrap();
    assert_eq!(
        repo.files[&pushes[0].1]["a.rs"],
        edits(&[("a.rs", "fn a() {}")])["a.rs"],
        "the rebase carried the format commit"
    );
}
