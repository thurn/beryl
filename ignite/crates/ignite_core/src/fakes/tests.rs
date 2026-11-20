use super::*;
use crate::config::Os;
use crate::ids::TreeId;
use crate::ids::{CheckName, InputHash, MachineId, RecordId};
use crate::protocol::Outcome;
use crate::traits::{BuildResult, Builds, Git};
use std::path::Path;

fn key() -> ResultKey {
    ResultKey {
        check: CheckName::new("rust:a::b").unwrap(),
        hash: InputHash::new("a".repeat(64)).unwrap(),
        platform: Some(Os::Macos),
    }
}

#[test]
fn the_hub_caches_what_it_stores_and_fails_when_down() {
    let hub = FakeHub::default();
    assert_eq!(hub.lookup(&[key()]).unwrap(), vec![None]);
    let machine = MachineId::new("m-1").unwrap();
    let new = NewResult {
        key: key(),
        outcome: Outcome::Pass,
        machine,
        record: RecordId(1),
        output: None,
        cost: crate::protocol::hub::ResultCost::default(),
        areas: Vec::new(),
    };
    hub.store(&[new]).unwrap();
    assert_eq!(
        hub.lookup(&[key()]).unwrap()[0].as_ref().unwrap().outcome,
        Outcome::Pass
    );
    hub.set_unreachable(true);
    assert!(matches!(hub.lookup(&[key()]), Err(Error::Unreachable(_))));
}

#[test]
fn leases_exclude_other_holders() {
    let hub = FakeHub::default();
    let acquire = |holder: &str| LeaseOp::Acquire {
        holder: holder.to_owned(),
        ttl_s: 60,
        payload: None,
        no_queue: true,
    };
    assert!(matches!(
        hub.lease("land", acquire("a")).unwrap(),
        LeaseReply::Held(_)
    ));
    assert!(matches!(
        hub.lease("land", acquire("b")).unwrap(),
        LeaseReply::Busy(_)
    ));
    let release = LeaseOp::Release {
        holder: "a".to_owned(),
        if_revision: None,
    };
    assert!(matches!(
        hub.lease("land", release).unwrap(),
        LeaseReply::Held(_)
    ));
    assert!(matches!(
        hub.lease("land", acquire("b")).unwrap(),
        LeaseReply::Held(_)
    ));
}

#[test]
fn the_feed_returns_changes_after_since() {
    let hub = FakeHub::default();
    hub.push_change("ticket", None);
    let second = hub.push_change("ticket", None);
    let batch = hub.feed(Seq(1), 0).unwrap();
    assert_eq!(batch.seq, second);
    assert_eq!(batch.changes.len(), 1);
}

#[test]
fn builds_are_built_once_then_current() {
    let builds = FakeBuilds::default();
    let tree = TreeId::new("tree-01").unwrap();
    assert!(matches!(
        builds.ensure(&tree, "tests", &[]),
        BuildResult::Built { .. }
    ));
    assert_eq!(builds.ensure(&tree, "tests", &[]), BuildResult::Current);
}

#[test]
fn the_launcher_answers_scripted_prefixes() {
    let launcher = FakeLauncher::default();
    let failing = RunResult {
        exit: Some(2),
        output: PathBuf::from("out"),
        usage: Usage::default(),
        hung: false,
    };
    launcher.script(&["cargo", "test"], failing);
    let spec = |args: &[&str]| CommandSpec {
        args: args.iter().map(|s| (*s).to_owned()).collect(),
        cwd: PathBuf::new(),
        env: Vec::new(),
        class: crate::protocol::Class::Agent,
        cores: 1,
        resources: Vec::new(),
        grows: false,
        tree: None,
        ticket: None,
        hang_limit_s: None,
        reserve: None,
    };
    assert_eq!(
        launcher.run(&spec(&["cargo", "test", "-p", "x"])).exit,
        Some(2)
    );
    assert_eq!(launcher.run(&spec(&["cargo", "build"])).exit, Some(0));
    assert_eq!(launcher.runs().len(), 2);
}

#[test]
fn git_pushes_move_the_tip() {
    let git = FakeGit::default();
    let commit = Commit::new("3a9e4c1").unwrap();
    git.push(Path::new("t"), &commit, "staging").unwrap();
    assert_eq!(git.tip("staging").unwrap(), commit);
    assert!(git.tip("master").is_err());
}

#[test]
fn verdicts_are_kept_by_key_and_scripted_replays_come_first_queued_first() {
    use crate::protocol::hub::VerdictKey;
    use crate::traits::{Git, Rebase};

    let commit = |c: char| Commit::new(c.to_string().repeat(40)).unwrap();
    let hub = FakeHub::default();
    let key = VerdictKey {
        tree: "f".repeat(40),
        base: commit('a'),
        settings: "s".to_owned(),
    };
    assert_eq!(hub.verdict(&key).unwrap(), None);
    let verdict = Verdict {
        key: key.clone(),
        head: commit('b'),
        machine: "m".to_owned(),
        passed_at: Timestamp::EPOCH,
        families: Vec::new(),
        left_to_tip: 0,
        always_not_run: Vec::new(),
    };
    hub.store_verdict(&verdict).unwrap();
    assert_eq!(hub.verdict(&key).unwrap(), Some(verdict));

    let git = FakeGit::default();
    let repo = std::path::Path::new("/repo");
    let conflict = Rebase::Conflict {
        files: vec!["a.rs".to_owned()],
    };
    git.script_replay(conflict.clone());
    assert_eq!(
        git.replay(repo, &commit('b'), &commit('c')).unwrap(),
        conflict
    );
    let Rebase::Clean { onto, .. } = git.replay(repo, &commit('b'), &commit('c')).unwrap() else {
        panic!("an unscripted replay is clean");
    };
    assert_eq!(onto, commit('c'));
    assert_eq!(
        git.merge_base(repo, &commit('b'), &commit('c')).unwrap(),
        commit('c')
    );
}
