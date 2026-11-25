//! Reading the lines commits changed and searching a commit's parent for words, against a
//! temporary repository, as a tip round does to find the later commits that use what a
//! reverted commit added.

mod support;

use std::collections::BTreeSet;
use std::fs;

use ignite_core::traits::{Git, LineChanges};
use ignite_git::GitCli;
use support::{Scratch, commit_all, init, write};

fn owned(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| (*w).to_owned()).collect()
}

/// A history in which commit A adds a function, B calls it and C changes something unrelated.
#[test]
fn a_function_one_commit_added_is_found_in_the_commit_that_calls_it() {
    let scratch = Scratch::new("words");
    let repo = scratch.path("repo");
    fs::create_dir_all(&repo).unwrap();
    init(&repo);
    write(
        &repo,
        "lib.rs",
        "pub fn land() -> Filter {\n    Filter::Land\n}\n",
    );
    write(&repo, "logo.png", "\u{0}\u{1}binary nonland\u{0}");
    commit_all(&repo, "base");
    write(
        &repo,
        "lib.rs",
        "pub fn land() -> Filter {\n    Filter::Land\n}\n\npub fn nonland() -> Filter {\n    \
         Filter::Not(land())\n}\n",
    );
    let a = commit_all(&repo, "A: add nonland");
    write(
        &repo,
        "card.rs",
        "fn target() -> Filter {\n    nonland()\n}\n",
    );
    let b = commit_all(&repo, "B: call nonland");
    write(
        &repo,
        "other.rs",
        "fn target() -> Filter {\n    land()\n}\n",
    );
    let c = commit_all(&repo, "C: unrelated");
    let git = GitCli {
        repo: repo.clone(),
        remote: "origin".to_owned(),
    };

    let added = git.line_changes(&repo, std::slice::from_ref(&a)).unwrap();
    assert_eq!(
        added,
        LineChanges {
            added: owned(&[
                "",
                "pub fn nonland() -> Filter {",
                "    Filter::Not(land())",
                "}"
            ]),
            removed: Vec::new(),
        }
    );
    // Before A, every word of its added lines but `nonland` and `Not` was in a text file; the
    // binary file that holds `nonland` does not count.
    let asked = owned(&["Filter", "Not", "fn", "land", "nonland", "pub"]);
    assert_eq!(
        git.words_before(&repo, &a, &asked).unwrap(),
        ["Filter", "fn", "land", "pub"]
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>()
    );
    assert!(
        git.line_changes(&repo, std::slice::from_ref(&b))
            .unwrap()
            .added
            .iter()
            .any(|line| line.contains("nonland"))
    );
    assert!(
        !git.line_changes(&repo, std::slice::from_ref(&c))
            .unwrap()
            .added
            .iter()
            .any(|line| line.contains("nonland"))
    );
    // Two commits read together give the change from the first one's parent to the last.
    let both = git.line_changes(&repo, &[a, b]).unwrap();
    assert_eq!(both.added.len(), 7);
}
