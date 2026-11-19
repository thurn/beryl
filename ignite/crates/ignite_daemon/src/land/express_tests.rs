use std::fs;
use std::path::Path;
use std::process::Command;

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
