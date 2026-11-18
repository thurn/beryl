//! Object IDs and worktrees, against temporary repositories.

mod support;

use std::fs;

use ignite_core::ids::Oid;
use ignite_git::{treehash, worktree};
use support::{Scratch, commit_all, git, head, init, write};

fn ids(tree: &std::path::Path, paths: &[&str]) -> Vec<(String, Oid)> {
    let paths: Vec<String> = paths.iter().map(|p| (*p).to_owned()).collect();
    treehash::object_ids(tree, &paths).unwrap()
}

fn id_of(tree: &std::path::Path, path: &str) -> Option<Oid> {
    ids(tree, &[path]).into_iter().next().map(|(_, oid)| oid)
}

/// The blob ID git gives `text`, computed without the tree.
fn blob(scratch: &Scratch, text: &str) -> Oid {
    write(&scratch.0, "probe", text);
    let oid = git(&scratch.0, &["hash-object", "probe"]);
    fs::remove_file(scratch.path("probe")).unwrap();
    Oid::new(oid.trim()).unwrap()
}

#[test]
fn object_ids_cover_staged_unstaged_and_untracked_files_but_not_ignored_output() {
    let scratch = Scratch::new("treehash");
    let tree = scratch.path("tree");
    let probe = Scratch::new("probe");
    fs::create_dir_all(&tree).unwrap();
    init(&tree);
    write(&tree, ".gitignore", "target/\n");
    write(&tree, "src/staged.txt", "old\n");
    write(&tree, "src/unstaged.txt", "old\n");
    let first = commit_all(&tree, "first");
    let committed_src = id_of(&tree, "src").unwrap();

    write(&tree, "src/staged.txt", "staged\n");
    git(&tree, &["add", "src/staged.txt"]);
    write(&tree, "src/unstaged.txt", "unstaged\n");
    write(&tree, "src/untracked.txt", "untracked\n");
    let index_before = fs::read(tree.join(".git/index")).unwrap();
    write(&tree, "target/out.bin", "build output\n");

    let found = ids(
        &tree,
        &[
            "src/staged.txt",
            "src/unstaged.txt",
            "src/untracked.txt",
            "target/out.bin",
            "target",
            "src/",
            ".",
        ],
    );
    let names: Vec<&str> = found.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        names,
        [
            "src/staged.txt",
            "src/unstaged.txt",
            "src/untracked.txt",
            "src/",
            "."
        ],
        "ignored build output has no object ID"
    );
    assert_eq!(found[0].1, blob(&probe, "staged\n"));
    assert_eq!(found[1].1, blob(&probe, "unstaged\n"));
    assert_eq!(found[2].1, blob(&probe, "untracked\n"));
    assert_ne!(
        found[3].1, committed_src,
        "the directory's tree ID follows its edits"
    );

    // Ignored output never changes a directory's ID; a second edit to an already-modified file
    // does, although `git status` prints the same line for it.
    let src = found[3].1.clone();
    write(&tree, "target/other.bin", "more output\n");
    assert_eq!(id_of(&tree, "src").unwrap(), src);
    write(&tree, "src/unstaged.txt", "edited again, longer\n");
    assert_eq!(
        id_of(&tree, "src/unstaged.txt").unwrap(),
        blob(&probe, "edited again, longer\n")
    );
    assert_ne!(id_of(&tree, "src").unwrap(), src);

    // Deleting a tracked file removes it from the result.
    fs::remove_file(tree.join("src/staged.txt")).unwrap();
    assert_eq!(id_of(&tree, "src/staged.txt"), None);

    // The branch, the index and the staged state are as the test left them.
    assert_eq!(fs::read(tree.join(".git/index")).unwrap(), index_before);
    assert_eq!(head(&tree), first);
    assert_eq!(git(&tree, &["branch", "--show-current"]).trim(), "main");
    let status = git(
        &tree,
        &[
            "--no-optional-locks",
            "status",
            "--porcelain",
            "--untracked-files=all",
        ],
    );
    assert_eq!(
        status,
        "MD src/staged.txt\n M src/unstaged.txt\n?? src/untracked.txt\n"
    );
    let leftovers: Vec<_> = fs::read_dir(tree.join(".git"))
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|name| name.starts_with("index."))
        .collect();
    assert!(
        leftovers.is_empty(),
        "temporary indexes left: {leftovers:?}"
    );
}

#[test]
fn reset_restores_a_clean_checkout_and_keeps_build_directories() {
    let scratch = Scratch::new("reset");
    let repo = scratch.path("repo");
    fs::create_dir_all(&repo).unwrap();
    init(&repo);
    write(&repo, ".gitignore", "target/\n*.log\n");
    write(&repo, "rust/src/lib.rs", "fn a() {}\n");
    let first = commit_all(&repo, "first");
    write(&repo, "rust/src/lib.rs", "fn b() {}\n");
    let second = commit_all(&repo, "second");

    let tree = scratch.path("trees/1");
    worktree::add(&repo, &tree, "work-1", &second).unwrap();
    write(&tree, "rust/src/lib.rs", "edited\n");
    write(&tree, "rust/target/debug/app", "binary\n");
    write(&tree, "junk.txt", "untracked\n");
    write(&tree, "run.log", "ignored but not kept\n");
    write(&tree, "other/target/x", "ignored, not a kept directory\n");

    worktree::reset(&tree, "work-2", &first, &["rust/target".to_owned()]).unwrap();

    assert_eq!(head(&tree), first);
    assert_eq!(git(&tree, &["branch", "--show-current"]).trim(), "work-2");
    assert_eq!(
        fs::read_to_string(tree.join("rust/src/lib.rs")).unwrap(),
        "fn a() {}\n"
    );
    assert_eq!(
        fs::read_to_string(tree.join("rust/target/debug/app")).unwrap(),
        "binary\n"
    );
    assert!(!tree.join("junk.txt").exists());
    assert!(!tree.join("run.log").exists());
    assert!(!tree.join("other").exists());
    assert_eq!(git(&tree, &["status", "--porcelain"]), "");

    let listed = worktree::list(&repo).unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].path, repo);
    assert_eq!(listed[0].branch.as_deref(), Some("main"));
    assert_eq!(listed[1].path, tree);
    assert_eq!(listed[1].head.as_ref(), Some(&first));
    assert_eq!(listed[1].branch.as_deref(), Some("work-2"));

    worktree::remove(&repo, &tree).unwrap();
    assert!(!tree.exists());
    assert_eq!(worktree::list(&repo).unwrap().len(), 1);

    // A tree deleted by hand can be added again at the same path.
    worktree::add(&repo, &tree, "work-1", &first).unwrap();
    fs::remove_dir_all(&tree).unwrap();
    worktree::add(&repo, &tree, "work-1", &second).unwrap();
    assert_eq!(head(&tree), second);
}

#[test]
fn reset_aborts_a_rebase_left_in_progress() {
    let scratch = Scratch::new("reset-rebase");
    let repo = scratch.path("repo");
    fs::create_dir_all(&repo).unwrap();
    init(&repo);
    write(&repo, "f.txt", "base\n");
    let base = commit_all(&repo, "base");
    write(&repo, "f.txt", "main\n");
    let main = commit_all(&repo, "main");
    git(&repo, &["checkout", "--quiet", "-b", "side", base.as_str()]);
    write(&repo, "f.txt", "side\n");
    commit_all(&repo, "side");
    let out = std::process::Command::new("git")
        .current_dir(&repo)
        .args(["rebase", main.as_str()])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "the rebase should stop on a conflict"
    );

    worktree::reset(&repo, "side", &main, &[]).unwrap();
    assert_eq!(head(&repo), main);
    assert!(!repo.join(".git/rebase-merge").exists());
    assert_eq!(git(&repo, &["status", "--porcelain"]), "");
}
