use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};

use super::*;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "{args:?}: {out:?}");
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

/// A repository with a `tool/` directory, and the express lane over it with `commands`.
fn lane(name: &str, commands: &[&str]) -> (PathBuf, MachineExpress, Commit) {
    let root = ignite_core::test_files::temp_dir()
        .join(format!("ignite-express-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let repo = root.join("repo");
    fs::create_dir_all(repo.join("tool")).unwrap();
    fs::write(repo.join("tool/lib.txt"), "one").unwrap();
    fs::write(repo.join(".gitignore"), "target/\n").unwrap();
    git(&repo, &["init", "--quiet", "-b", "main"]);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "--quiet", "-m", "one"]);
    let commit = Commit::new(git(&repo, &["rev-parse", "HEAD"])).unwrap();
    let lane = MachineExpress {
        repo: repo.clone(),
        tree: repo.join("trees/express"),
        dir: "tool".to_owned(),
        commands: commands.iter().map(|c| (*c).to_owned()).collect(),
        cores: 2,
        memory: None,
        lender: None,
    };
    (root, lane, commit)
}

#[test]
fn the_commands_run_in_the_tree_at_the_commit_and_its_target_directory_is_kept() {
    let (root, lane, first) = lane("pass", &["true", "true"]);
    let run = lane.run(&first);
    assert!(run.passed(), "{run:?}");
    assert_eq!(run.ran.len(), 2);
    assert_eq!(run.cores, 2);
    let tool = lane.tree.join("tool");
    assert_eq!(fs::read_to_string(tool.join("lib.txt")).unwrap(), "one");
    fs::write(tool.join("target/warm"), "build").unwrap();
    fs::write(tool.join("stray.txt"), "left over").unwrap();

    fs::write(lane.repo.join("tool/lib.txt"), "two").unwrap();
    git(&lane.repo, &["commit", "--quiet", "-am", "two"]);
    let second = Commit::new(git(&lane.repo, &["rev-parse", "HEAD"])).unwrap();
    assert!(lane.run(&second).passed());
    assert_eq!(fs::read_to_string(tool.join("lib.txt")).unwrap(), "two");
    assert!(
        tool.join("target/warm").exists(),
        "the build directory is kept"
    );
    assert!(!tool.join("stray.txt").exists(), "other files are cleaned");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_failing_command_stops_the_run_and_reports_its_output() {
    let (root, lane, commit) = lane("fail", &["ls no-such-file", "true"]);
    let run = lane.run(&commit);
    let (command, output) = run.failure.clone().unwrap();
    assert_eq!(command, "ls no-such-file");
    assert!(output.contains("no-such-file"), "{output}");
    assert_eq!(
        run.ran.len(),
        1,
        "the command after the failure did not run"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_run_in_an_agents_tree_uses_that_tree_as_it_is() {
    let (root, lane, _) = lane("in-tree", &["test -f lib.txt", "touch ran-here"]);
    let agent = lane.repo.clone();
    fs::write(agent.join("tool/uncommitted.txt"), "edit").unwrap();
    let run = lane.run_in(&agent, &section(&lane), &[]);
    assert!(run.passed(), "{run:?}");
    assert_eq!(run.ran.len(), 2);
    assert!(agent.join("tool/ran-here").exists());
    assert!(agent.join("tool/target/express.log").exists());
    assert!(
        agent.join("tool/uncommitted.txt").exists(),
        "nothing is checked out or cleaned"
    );
    assert!(!lane.tree.exists(), "the express tree is not made");
    let _ = fs::remove_dir_all(root);
}

/// The lane `lane` with a pass memory over an in-memory database and a clock at `now`.
fn remembering(lane: &mut MachineExpress, clock: &Arc<ignite_core::fakes::FakeClock>) {
    let db = ignite_store::local::open_in_memory().unwrap();
    lane.memory = Some(PassMemory::new(Arc::new(Mutex::new(db)), clock.clone()));
}

/// The section a lane's own settings make, as the daemon's `ignite.toml` would declare it.
fn section(lane: &MachineExpress) -> ignite_core::config::Express {
    ignite_core::config::Express {
        paths: vec!["tool/**".to_owned()],
        tree: PathBuf::from("trees/express"),
        dir: lane.dir.clone(),
        commands: lane.commands.clone(),
        families: Vec::new(),
        inert: Vec::new(),
    }
}

#[test]
fn a_second_run_of_an_unchanged_clean_tree_skips_the_commands() {
    let (root, mut lane, _) = lane("reuse", &["true"]);
    let clock = Arc::new(ignite_core::fakes::FakeClock::new(
        ignite_core::time::Timestamp::parse("2026-11-18T10:00:00Z").unwrap(),
    ));
    remembering(&mut lane, &clock);
    let first = lane.run_in(&lane.repo, &section(&lane), &[]);
    assert!(
        first.passed() && first.reused_after_s.is_none(),
        "{first:?}"
    );
    assert_eq!(first.ran.len(), 1);
    clock.advance(240.0);
    let second = lane.run_in(&lane.repo, &section(&lane), &[]);
    assert!(second.passed() && second.ran.is_empty(), "{second:?}");
    assert_eq!(second.reused_after_s, Some(240.0));
    assert!(second.line().1.contains("passed for this tree 4 min ago"));
    // A new commit with other content runs the commands again.
    fs::write(lane.repo.join("tool/lib.txt"), "two").unwrap();
    git(&lane.repo, &["commit", "--quiet", "-am", "two"]);
    assert_eq!(lane.run_in(&lane.repo, &section(&lane), &[]).ran.len(), 1);
    // An edited `[express]` section does not match the earlier pass.
    let again = lane.run_in(&lane.repo, &section(&lane), &[]);
    assert!(again.reused_after_s.is_some());
    lane.commands.push("true".to_owned());
    remembering(&mut lane, &clock);
    assert_eq!(lane.run_in(&lane.repo, &section(&lane), &[]).ran.len(), 2);
    let _ = fs::remove_dir_all(root);
}

/// A lane over `name`'s repository whose tree passed the commands at 10:00 with an uncommitted
/// edit to its own file `tool/lib.txt`, as `ignite test` before a commit, and its clock four
/// minutes later. The pass left the tree's own index as it was.
fn tested_dirty(name: &str) -> (PathBuf, MachineExpress, ignite_git::GitCli) {
    let (root, mut lane, _) = lane(name, &["true"]);
    let clock = Arc::new(ignite_core::fakes::FakeClock::new(
        ignite_core::time::Timestamp::parse("2026-11-18T10:00:00Z").unwrap(),
    ));
    remembering(&mut lane, &clock);
    fs::write(lane.repo.join("tool/lib.txt"), "the change").unwrap();
    git(&lane.repo, &["status", "--porcelain"]);
    let index = fs::read(lane.repo.join(".git/index")).unwrap();
    let run = lane.run_in(&lane.repo, &section(&lane), &own());
    assert_eq!(run.ran.len(), 1, "{run:?}");
    assert_eq!(fs::read(lane.repo.join(".git/index")).unwrap(), index);
    assert_eq!(
        git(&lane.repo, &["status", "--porcelain"]),
        "M tool/lib.txt"
    );
    clock.advance(240.0);
    let git = ignite_git::GitCli {
        repo: lane.repo.clone(),
        remote: "origin".to_owned(),
    };
    (root, lane, git)
}

#[test]
fn a_pass_in_a_dirty_tree_is_reused_once_exactly_those_edits_are_committed() {
    let (root, lane, _) = tested_dirty("dirty-commit");
    git(&lane.repo, &["commit", "--quiet", "-am", "the change"]);
    let run = lane.run_in(&lane.repo, &section(&lane), &own());
    assert!(run.passed() && run.ran.is_empty(), "{run:?}");
    assert_eq!(run.reused_after_s, Some(240.0));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_pass_in_a_dirty_tree_is_not_reused_after_a_further_edit() {
    let (root, lane, _) = tested_dirty("dirty-edit");
    fs::write(lane.repo.join("tool/lib.txt"), "the change, edited").unwrap();
    assert_eq!(
        lane.run_in(&lane.repo, &section(&lane), &own()).ran.len(),
        1
    );
    // Committed, the further edit still does not match the first pass, only the second.
    fs::write(lane.repo.join("tool/lib.txt"), "the change, edited again").unwrap();
    git(&lane.repo, &["commit", "--quiet", "-am", "the change"]);
    assert_eq!(
        lane.run_in(&lane.repo, &section(&lane), &own()).ran.len(),
        1
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_landing_reuses_a_dirty_trees_pass_after_the_commit_and_an_unrelated_landing() {
    let (root, lane, git_cli) = tested_dirty("dirty-landed");
    // The landed commit goes in first, as a rebase would put it under the change.
    let edit = fs::read_to_string(lane.repo.join("tool/lib.txt")).unwrap();
    git(&lane.repo, &["checkout", "--quiet", "--", "tool/lib.txt"]);
    commit_files(&lane.repo, &["tool/other.txt"], "another Ignite change");
    let head = commit_files(&lane.repo, &["tool/lib.txt"], &edit);
    let run = landing_reuses(&lane, &git_cli, &head, &own()).expect("the pass holds");
    assert!(run.passed() && run.ran.is_empty(), "{run:?}");
    assert_eq!(run.reused_after_s, Some(240.0));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_pass_older_than_an_hour_is_not_reused() {
    let (root, mut lane, _) = lane("expiry", &["true"]);
    let clock = Arc::new(ignite_core::fakes::FakeClock::new(
        ignite_core::time::Timestamp::parse("2026-11-18T10:00:00Z").unwrap(),
    ));
    remembering(&mut lane, &clock);
    assert_eq!(lane.run_in(&lane.repo, &section(&lane), &[]).ran.len(), 1);
    clock.advance(3601.0);
    assert_eq!(lane.run_in(&lane.repo, &section(&lane), &[]).ran.len(), 1);
    let _ = fs::remove_dir_all(root);
}

/// Commits `files`, each written with `text`, in `repo` and returns the new commit.
fn commit_files(repo: &Path, files: &[&str], text: &str) -> Commit {
    for file in files {
        fs::write(repo.join(file), text).unwrap();
    }
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "--quiet", "-m", text]);
    Commit::new(git(repo, &["rev-parse", "HEAD"])).unwrap()
}

/// A lane over `name`'s repository whose clean tree passed the commands at 10:00 with the
/// change's own file `tool/lib.txt`, its clock four minutes later, and the commit it passed on.
fn checked(name: &str) -> (PathBuf, MachineExpress, ignite_git::GitCli, Commit) {
    let (root, mut lane, _) = lane(name, &["true"]);
    let clock = Arc::new(ignite_core::fakes::FakeClock::new(
        ignite_core::time::Timestamp::parse("2026-11-18T10:00:00Z").unwrap(),
    ));
    remembering(&mut lane, &clock);
    let checked = commit_files(&lane.repo, &["tool/lib.txt"], "the change");
    assert_eq!(
        lane.run_in(&lane.repo, &section(&lane), &own()).ran.len(),
        1
    );
    clock.advance(240.0);
    let git = ignite_git::GitCli {
        repo: lane.repo.clone(),
        remote: "origin".to_owned(),
    };
    (root, lane, git, checked)
}

/// The change's own files in [`checked`].
fn own() -> Vec<String> {
    vec!["tool/lib.txt".to_owned()]
}

/// [`reused`] for a landing of `lane`'s repository at `head` whose own files are `changed`.
fn landing_reuses(
    lane: &MachineExpress,
    git: &ignite_git::GitCli,
    head: &Commit,
    changed: &[String],
) -> Option<ExpressRun> {
    reused(lane, git, &section(lane), &lane.repo, head, changed).unwrap()
}

#[test]
fn a_landing_reuses_a_checks_pass_when_the_commits_landed_since_touch_none_of_its_files() {
    let (root, lane, git, checked) = checked("reuse-landed");
    let head = commit_files(&lane.repo, &["tool/other.txt"], "another Ignite change");
    let run = landing_reuses(&lane, &git, &head, &own()).expect("the pass holds");
    assert!(run.passed() && run.ran.is_empty(), "{run:?}");
    assert_eq!(run.reused_after_s, Some(240.0));
    assert_eq!(run.reused_on.as_ref(), Some(&checked));
    let text = run.line().1;
    let expected = format!(
        "passed for this tree 4 min ago on {}; the commits landed since touch none of its files",
        super::super::short(&checked)
    );
    assert!(text.contains(&expected), "{text}");
    // The landing's own files must be those the pass ran with.
    let more = vec!["tool/lib.txt".to_owned(), "tool/other.txt".to_owned()];
    assert_eq!(landing_reuses(&lane, &git, &head, &more), None);
    // On the very commit that passed, the tree ID finds the pass instead.
    assert_eq!(landing_reuses(&lane, &git, &checked, &own()), None);
    // A pass of another section is not reused.
    let mut other = section(&lane);
    other.commands.push("true".to_owned());
    assert_eq!(
        reused(&lane, &git, &other, &lane.repo, &head, &own()).unwrap(),
        None
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_landing_reruns_when_a_landed_commit_touches_one_of_the_changes_files() {
    let (root, lane, git, _) = checked("rerun-own");
    let head = commit_files(
        &lane.repo,
        &["tool/lib.txt"],
        "a landed edit to the same file",
    );
    assert_eq!(landing_reuses(&lane, &git, &head, &own()), None);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_landing_reruns_when_a_landed_commit_touches_the_workspaces_lock_file() {
    let (root, lane, git, _) = checked("rerun-lock");
    let head = commit_files(&lane.repo, &["tool/Cargo.lock"], "a new dependency");
    assert_eq!(landing_reuses(&lane, &git, &head, &own()), None);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_landing_reuses_a_pass_when_the_only_file_a_landed_commit_shares_with_it_is_inert() {
    let (root, mut lane, _) = lane("reuse-inert", &["true"]);
    let clock = Arc::new(ignite_core::fakes::FakeClock::new(
        ignite_core::time::Timestamp::parse("2026-11-19T21:40:00Z").unwrap(),
    ));
    remembering(&mut lane, &clock);
    let mut express = section(&lane);
    express.inert = vec!["notes.md".to_owned()];
    let files = vec!["notes.md".to_owned(), "tool/lib.txt".to_owned()];
    let checked = commit_files(&lane.repo, &["notes.md", "tool/lib.txt"], "the change");
    assert!(lane.run_in(&lane.repo, &express, &files).passed());
    clock.advance(60.0);
    let git = ignite_git::GitCli {
        repo: lane.repo.clone(),
        remote: "origin".to_owned(),
    };
    // Another landed change also edited the notes, which no command builds.
    let head = commit_files(&lane.repo, &["notes.md"], "another change's notes");
    let run = reused(&lane, &git, &express, &lane.repo, &head, &files)
        .unwrap()
        .expect("the pass holds");
    assert_eq!(run.reused_on.as_ref(), Some(&checked));
    // Without the pattern the shared file runs the commands again.
    let strict = section(&lane);
    assert!(lane.run_in(&lane.repo, &strict, &files).passed());
    let later = commit_files(&lane.repo, &["notes.md"], "more notes");
    assert_eq!(
        reused(&lane, &git, &strict, &lane.repo, &later, &files).unwrap(),
        None
    );
    // A shared file that is not inert still runs them.
    let built = commit_files(&lane.repo, &["tool/lib.txt"], "a landed edit to the code");
    assert_eq!(
        reused(&lane, &git, &express, &lane.repo, &built, &files).unwrap(),
        None
    );
    let _ = fs::remove_dir_all(root);
}

/// A lender with `free` cores that records what was borrowed and returned.
struct FakeLender {
    free: u32,
    log: std::sync::Mutex<Vec<String>>,
}

impl CoreLender for FakeLender {
    fn borrow(&self, wanted: u32) -> Option<Loan> {
        let cores = wanted.min(self.free);
        self.log.lock().unwrap().push(format!("borrow {wanted}"));
        (cores > 0).then_some(Loan { grant: 7, cores })
    }

    fn give_back(&self, loan: &Loan) {
        self.log
            .lock()
            .unwrap()
            .push(format!("return {}", loan.grant));
    }
}

#[test]
fn free_cores_are_borrowed_for_the_run_and_returned_after_it() {
    let (root, mut lane, commit) = lane("borrow", &["env"]);
    let lender = Arc::new(FakeLender {
        free: 20,
        log: std::sync::Mutex::new(Vec::new()),
    });
    lane.lender = Some(lender.clone());
    let run = lane.run(&commit);
    assert!(run.passed(), "{run:?}");
    // 2 reserved cores, and the lender is asked for up to `MOST_CORES` in all.
    assert_eq!(run.cores, MOST_CORES);
    let log = fs::read_to_string(lane.tree.join("tool/target/express.log")).unwrap();
    assert!(
        log.contains(&format!("IGNITE_CORES={MOST_CORES}\n")),
        "{log}"
    );
    assert!(
        log.contains(&format!("CARGO_BUILD_JOBS={MOST_CORES}\n")),
        "{log}"
    );
    assert_eq!(
        *lender.log.lock().unwrap(),
        [format!("borrow {}", MOST_CORES - 2), "return 7".to_owned()]
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn with_no_free_cores_the_run_keeps_its_reserved_cores_and_returns_nothing() {
    let (root, mut lane, commit) = lane("no-borrow", &["true"]);
    let lender = Arc::new(FakeLender {
        free: 0,
        log: std::sync::Mutex::new(Vec::new()),
    });
    lane.lender = Some(lender.clone());
    let run = lane.run(&commit);
    assert!(run.passed(), "{run:?}");
    assert_eq!(run.cores, 2);
    assert_eq!(
        lender.log.lock().unwrap().len(),
        1,
        "asked once, nothing to return"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_run_in_uses_the_commands_of_the_section_it_is_given() {
    let (root, mut lane, _) = lane("own-section", &["false"]);
    let clock = Arc::new(ignite_core::fakes::FakeClock::new(
        ignite_core::time::Timestamp::parse("2026-11-18T10:00:00Z").unwrap(),
    ));
    remembering(&mut lane, &clock);
    // The lane's own commands fail, but the tree's section names others.
    let mut own = section(&lane);
    own.commands = vec!["touch own-ran".to_owned()];
    let run = lane.run_in(&lane.repo, &own, &[]);
    assert!(run.passed(), "{run:?}");
    assert!(lane.repo.join("tool/own-ran").exists());
    // A pass of one section is not reused for another.
    own.commands = vec!["true".to_owned(), "true".to_owned()];
    assert_eq!(lane.run_in(&lane.repo, &own, &[]).ran.len(), 2);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn the_tree_s_ignite_toml_is_read_for_its_express_section_with_the_daemons_as_fallback() {
    let root =
        ignite_core::test_files::temp_dir().join(format!("ignite-section-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let daemons = ignite_core::config::Express {
        paths: vec!["a/**".to_owned()],
        tree: PathBuf::from("t"),
        dir: "a".to_owned(),
        commands: vec!["daemon".to_owned()],
        families: Vec::new(),
        inert: Vec::new(),
    };
    // No file, then a file that does not parse: the daemon's section.
    assert_eq!(section_in(&root, Some(&daemons)), Some(daemons.clone()));
    fs::write(root.join("ignite.toml"), "[express").unwrap();
    assert_eq!(section_in(&root, Some(&daemons)), Some(daemons.clone()));
    assert_eq!(section_in(&root, None), None);
    let _ = fs::remove_dir_all(root);
}
