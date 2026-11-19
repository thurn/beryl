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
    let root = std::env::temp_dir().join(format!("ignite-express-{name}-{}", std::process::id()));
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
    let run = lane.run_in(&agent);
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
    let express = ignite_core::config::Express {
        paths: vec!["tool/**".to_owned()],
        tree: PathBuf::from("trees/express"),
        dir: "tool".to_owned(),
        commands: lane.commands.clone(),
        families: Vec::new(),
    };
    let db = ignite_store::local::open_in_memory().unwrap();
    lane.memory = Some(PassMemory::new(
        Arc::new(Mutex::new(db)),
        clock.clone(),
        &express,
        lane.cores,
    ));
}

#[test]
fn a_second_run_of_an_unchanged_clean_tree_skips_the_commands() {
    let (root, mut lane, _) = lane("reuse", &["true"]);
    let clock = Arc::new(ignite_core::fakes::FakeClock::new(
        ignite_core::time::Timestamp::parse("2026-11-18T10:00:00Z").unwrap(),
    ));
    remembering(&mut lane, &clock);
    let first = lane.run_in(&lane.repo);
    assert!(
        first.passed() && first.reused_after_s.is_none(),
        "{first:?}"
    );
    assert_eq!(first.ran.len(), 1);
    clock.advance(240.0);
    let second = lane.run_in(&lane.repo);
    assert!(second.passed() && second.ran.is_empty(), "{second:?}");
    assert_eq!(second.reused_after_s, Some(240.0));
    assert!(second.line().1.contains("passed for this tree 4 min ago"));
    // A new commit with other content runs the commands again.
    fs::write(lane.repo.join("tool/lib.txt"), "two").unwrap();
    git(&lane.repo, &["commit", "--quiet", "-am", "two"]);
    assert_eq!(lane.run_in(&lane.repo).ran.len(), 1);
    // An edited `[express]` section does not match the earlier pass.
    let again = lane.run_in(&lane.repo);
    assert!(again.reused_after_s.is_some());
    lane.commands.push("true".to_owned());
    remembering(&mut lane, &clock);
    assert_eq!(lane.run_in(&lane.repo).ran.len(), 2);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_dirty_tree_neither_records_nor_reuses_a_pass() {
    let (root, mut lane, _) = lane("dirty", &["true"]);
    let clock = Arc::new(ignite_core::fakes::FakeClock::new(
        ignite_core::time::Timestamp::parse("2026-11-18T10:00:00Z").unwrap(),
    ));
    remembering(&mut lane, &clock);
    fs::write(lane.repo.join("tool/lib.txt"), "edited").unwrap();
    assert_eq!(lane.run_in(&lane.repo).ran.len(), 1);
    assert_eq!(lane.run_in(&lane.repo).ran.len(), 1);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_pass_older_than_an_hour_is_not_reused() {
    let (root, mut lane, _) = lane("expiry", &["true"]);
    let clock = Arc::new(ignite_core::fakes::FakeClock::new(
        ignite_core::time::Timestamp::parse("2026-11-18T10:00:00Z").unwrap(),
    ));
    remembering(&mut lane, &clock);
    assert_eq!(lane.run_in(&lane.repo).ran.len(), 1);
    clock.advance(3601.0);
    assert_eq!(lane.run_in(&lane.repo).ran.len(), 1);
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
