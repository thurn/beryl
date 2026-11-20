//! Landings against the fakes in [`fakes`], with no real time.

mod base;
mod express;
mod fakes;
mod format;
mod more;
mod races;
mod release;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ignite_core::config::Config;
use ignite_core::fakes::{FakeBuilds, FakeClock, FakeHub, FakeLauncher};
use ignite_core::ids::{Commit, MachineId, SessionId, TicketId, TreeId};
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::protocol::hub::Landed;
use ignite_core::protocol::tickets::{LandedWork, LeaseOp, LeaseReply};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Clock, Hub};

use super::env::{GateTree, TicketTree};
use super::records::{LandingRecord, RecordState, Records};
use super::{LandVerb, Landing, SyncVerb};
use crate::server::{Context, Verb};
use fakes::{CacheGate, LeaseHub, TestGit, TestTrees};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct World {
    ctx: Context,
    git: Arc<TestGit>,
    hub: Arc<LeaseHub>,
    clock: Arc<FakeClock>,
    trees: Arc<TestTrees>,
    gate: Arc<CacheGate>,
    landing: Arc<Landing>,
    records: PathBuf,
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.records);
    }
}

fn world() -> World {
    world_with_checks(vec![
        ("unit:a", vec!["a.txt"]),
        ("unit:b", vec!["b.txt"]),
        ("unit:both", vec!["a.txt", "b.txt"]),
        ("unit:c", vec!["c.txt"]),
    ])
}

fn world_with_checks(checks: Vec<(&'static str, Vec<&'static str>)>) -> World {
    world_with(checks, Vec::new())
}

/// A world whose gate runs `checks`, and whose `run_family` also knows `family_only`.
fn world_with(
    checks: Vec<(&'static str, Vec<&'static str>)>,
    family_only: Vec<&'static str>,
) -> World {
    let config = Config::parse(
        "ignite.toml",
        r#"
        [repository]
        remote = "origin"
        staging = "staging"
        master = "master"
        trees = "trees"
        checks = "checks"
        [hub]
        url = "http://localhost:1"
        project = "test"
        [gate]
        threshold = 0.004
        cap = 400
        [machine.local]
        ticket_trees = 2
        landing_trees = 1
        "#,
    )
    .unwrap();
    let clock = Arc::new(FakeClock::new(Timestamp::EPOCH));
    let hub = Arc::new(LeaseHub {
        hub: FakeHub::default(),
        clock: clock.clone(),
        leases: Mutex::new(HashMap::new()),
        landed_down: AtomicBool::new(false),
        steal_after_acquire: AtomicBool::new(false),
    });
    let git = Arc::new(TestGit::new(&[
        ("a.txt", "a"),
        ("b.txt", "b"),
        ("c.txt", "c"),
    ]));
    let trees = Arc::new(TestTrees {
        git: git.clone(),
        tickets: Mutex::new(HashMap::new()),
        branches: Mutex::new(HashMap::new()),
        uncommitted: Mutex::new(HashMap::new()),
        landing: Mutex::new(vec![GateTree {
            tree: TreeId::new("landing-1").unwrap(),
            path: PathBuf::from("/trees/landing-1"),
        }]),
        released: Mutex::new(Vec::new()),
        deleted: Mutex::new(Vec::new()),
        ..TestTrees::default()
    });
    let gate = Arc::new(CacheGate {
        checks,
        family_only,
        ..CacheGate::default()
    });
    let n = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
    let records = ignite_core::test_files::temp_dir().join(format!("ignite-land-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&records);
    let pause_clock = clock.clone();
    let landing = Arc::new(Landing {
        trees: trees.clone(),
        gate: gate.clone(),
        records: Records::new(&records),
        express: None,
        format: None,
        pause: Arc::new(move |seconds| pause_clock.advance(seconds)),
    });
    let ctx = Context {
        config,
        repo: PathBuf::from("/repo"),
        machine: MachineId::new("m-1").unwrap(),
        clock: clock.clone(),
        hub: hub.clone(),
        git: git.clone(),
        launcher: Arc::new(FakeLauncher::default()),
        builds: Arc::new(FakeBuilds::default()),
        session: None,
        tree: None,
    };
    World {
        ctx,
        git,
        hub,
        clock,
        trees,
        gate,
        landing,
        records,
    }
}

impl World {
    /// Leases `tree` to `ticket` for `session_name`, with `changes` committed on its branch.
    fn hold(&self, tree: &str, ticket: &str, session_name: &str, changes: &[(&str, &str)]) {
        let path = PathBuf::from(format!("/trees/{tree}"));
        self.git.start(&path, changes);
        let held = TicketTree {
            tree: TreeId::new(tree).unwrap(),
            path,
            ticket: TicketId::new(ticket).unwrap(),
            session: Some(SessionId::new(session_name).unwrap()),
            branch: Some(format!("ticket/{ticket}")),
        };
        let mut tickets = self.trees.tickets.lock().unwrap();
        tickets.insert(held.tree.clone(), held);
    }

    fn request(&self, verb: &str, tree: &str, session_name: &str, args: &[&str]) -> Request {
        let _ = self;
        Request {
            verb: verb.to_owned(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            cwd: PathBuf::from(format!("/trees/{tree}")),
            session: Some(SessionId::new(session_name).unwrap()),
            wait_s: 1,
            pid: None,
        }
    }

    fn land_as(&self, tree: &str, session_name: &str) -> Reply {
        let req = self.request("land", tree, session_name, &["-m", "the note"]);
        let mut ctx = self.ctx.clone();
        ctx.session.clone_from(&req.session);
        ctx.tree = Some(TreeId::new(tree).unwrap());
        LandVerb {
            landing: self.landing.clone(),
        }
        .run(&ctx, &req)
    }

    fn landings(&self) -> Vec<Landed> {
        self.hub.hub.landings()
    }

    fn queue_hook(&self, hook: impl FnOnce() + Send + 'static) {
        self.gate.hooks.lock().unwrap().push_back(Box::new(hook));
    }
}

#[test]
fn two_landings_on_one_tip_both_land_and_the_second_reruns_only_checks_reading_both() {
    let world = Arc::new(world());
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "a by t.1")]);
    world.hold("tree-02", "t.2", "local_b", &[("b.txt", "b by t.2")]);
    let first = world.clone();
    let first_reply = Arc::new(Mutex::new(None));
    let slot = first_reply.clone();
    // While the second landing runs its gate, the first lands.
    world.queue_hook(move || *slot.lock().unwrap() = Some(first.land_as("tree-01", "local_a")));

    let second = world.land_as("tree-02", "local_b");
    let first = first_reply.lock().unwrap().take().unwrap();

    assert_eq!(first.exit, Exit::Success, "{}", first.text);
    assert_eq!(second.exit, Exit::Success, "{}", second.text);
    assert!(second.text.contains("in pass 2 ("), "{}", second.text);
    let passes = world.gate.passes.lock().unwrap().clone();
    assert_eq!(passes.len(), 3, "{passes:?}");
    assert_eq!(passes[2], ["unit:both"]);
    let landed: Vec<String> = world
        .landings()
        .iter()
        .map(|l| l.ticket.to_string())
        .collect();
    assert_eq!(landed, ["t.1", "t.2"]);
    let pushes = world.git.pushes();
    assert_eq!(pushes.len(), 2);
    assert_eq!(world.git.parent_of(&pushes[1].1), Some(pushes[0].1.clone()));
    assert_eq!(world.hub.holder("staging"), None);
    assert_eq!(world.trees.released.lock().unwrap().len(), 2);
}

#[test]
/// A landing that lost three races used to hold the lease through its next gate, and every
/// other landing then waited for that gate. The lease is now never held through a gate: the
/// other machine lands on every pass, and this landing gets through on the first pass in which
/// the tip stays put.
fn a_landing_that_loses_races_never_holds_the_lease_through_a_gate() {
    let world = Arc::new(world());
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    let outcomes = Arc::new(Mutex::new(Vec::new()));
    for n in 0..4 {
        let (world2, outcomes) = (world.clone(), outcomes.clone());
        // Another machine lands whenever it can get the lease.
        world.queue_hook(move || {
            let op = LeaseOp::Acquire {
                holder: "m-2:other".to_owned(),
                ttl_s: 180,
                payload: None,
                no_queue: true,
            };
            match world2.hub.lease("staging", op).unwrap() {
                LeaseReply::Held(_) => {
                    world2.git.land_other(&[("c.txt", &format!("other {n}"))]);
                    let release = LeaseOp::Release {
                        holder: "m-2:other".to_owned(),
                        if_revision: None,
                    };
                    world2.hub.lease("staging", release).unwrap();
                    outcomes.lock().unwrap().push("landed");
                }
                LeaseReply::Busy(_) => outcomes.lock().unwrap().push("blocked"),
            }
        });
    }

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert_eq!(
        *outcomes.lock().unwrap(),
        ["landed", "landed", "landed", "landed"]
    );
    assert_eq!(world.gate.passes.lock().unwrap().len(), 5);
    assert!(reply.text.contains("in pass 5 ("), "{}", reply.text);
    assert_eq!(world.git.pushes().len(), 1);
    assert_eq!(world.hub.holder("staging"), None);
}

#[test]
fn a_failed_gate_exits_2_and_leaves_staging_and_the_ticket_as_they_were() {
    let world = world();
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "broken")]);
    world.git.land_other(&[("c.txt", "meanwhile")]);
    world
        .gate
        .failing
        .lock()
        .unwrap()
        .insert("unit:a".to_owned());
    let tip = world.git.tip_now();

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::CheckFailed, "{}", reply.text);
    assert_eq!(world.git.tip_now(), tip);
    assert!(world.git.pushes().is_empty());
    assert!(world.landings().is_empty());
    assert_eq!(world.hub.holder("staging"), None);
    assert!(reply.text.contains("FAIL     unit:a"), "{}", reply.text);
    assert!(reply.text.contains("src/a.rs:3"), "{}", reply.text);
    assert!(!reply.text.contains("/trees/"), "{}", reply.text);
    assert!(
        reply.text.contains("rerun    ignite test unit:a"),
        "{}",
        reply.text
    );
    assert!(world.trees.released.lock().unwrap().is_empty());
    assert_eq!(world.trees.landing.lock().unwrap().len(), 1);
    assert!(world.landing.records.all().unwrap().is_empty());
}

#[test]
fn a_dead_holders_lease_is_taken_over_once_it_expires() {
    let world = world();
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    world.hub.leases.lock().unwrap().insert(
        "staging".to_owned(),
        ("m-2:dead".to_owned(), Timestamp::EPOCH.plus_seconds(180.0)),
    );

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    let waited = world.clock.now().seconds_since(Timestamp::EPOCH);
    assert!((180.0..=190.0).contains(&waited), "waited {waited} s");
    assert_eq!(world.git.pushes().len(), 1);
}

#[test]
fn a_push_while_the_hub_is_down_closes_the_ticket_when_it_returns_with_one_push() {
    let world = world();
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    world.hub.landed_down.store(true, Ordering::SeqCst);

    let reply = world.land_as("tree-01", "local_a");
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(
        reply.text.contains("closes when the hub returns"),
        "{}",
        reply.text
    );
    assert!(world.landings().is_empty());
    let records = world.landing.records.all().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].state, RecordState::Pushed);
    assert_eq!(records[0].landed.work.note, "the note");

    // Landing again while the record waits does not push again.
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    let again = world.land_as("tree-01", "local_a");
    assert_eq!(again.exit, Exit::Success, "{}", again.text);
    assert_eq!(world.git.pushes().len(), 1);
    assert!(world.landings().is_empty());

    world.hub.landed_down.store(false, Ordering::SeqCst);
    world.landing.deliver_pending(&world.ctx);
    let landed: Vec<String> = world
        .landings()
        .iter()
        .map(|l| l.ticket.to_string())
        .collect();
    assert_eq!(landed, ["t.1"]);
    assert!(world.landing.records.all().unwrap().is_empty());
    assert_eq!(world.git.pushes().len(), 1);
}

#[test]
fn a_tree_on_another_branch_is_refused() {
    let world = world();
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    let path = PathBuf::from("/trees/tree-01");
    let mut branches = world.trees.branches.lock().unwrap();
    branches.insert(path, "ticket/t.9".to_owned());
    drop(branches);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Usage, "{}", reply.text);
    assert!(
        reply.text.contains("on branch ticket/t.9"),
        "{}",
        reply.text
    );
    assert!(world.git.pushes().is_empty());
}

#[test]
fn files_neither_committed_nor_ignored_are_named_in_the_refusal() {
    let world = world();
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    let mut uncommitted = world.trees.uncommitted.lock().unwrap();
    uncommitted.insert(
        PathBuf::from("/trees/tree-01"),
        vec!["scratch.log".to_owned()],
    );
    drop(uncommitted);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Usage, "{}", reply.text);
    assert!(reply.text.contains("scratch.log"), "{}", reply.text);
    assert!(world.git.pushes().is_empty());
}

#[test]
fn a_session_that_does_not_hold_the_ticket_is_refused_with_code_3() {
    let world = world();
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    let reply = world.land_as("tree-01", "local_z");
    assert_eq!(reply.exit, Exit::Conflict, "{}", reply.text);
    assert!(world.git.pushes().is_empty());
}

#[test]
fn a_rebase_conflict_exits_3_with_the_files() {
    let world = world();
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    let mut repo = world.git.repo.lock().unwrap();
    repo.conflicts
        .insert(PathBuf::from("/trees/tree-01"), vec!["a.txt".to_owned()]);
    drop(repo);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Conflict, "{}", reply.text);
    assert!(reply.text.contains("a.txt"), "{}", reply.text);
    assert_eq!(world.hub.holder("staging"), None);
}

#[test]
fn an_unreachable_remote_exits_4_and_the_next_landing_pushes_once() {
    let world = world();
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    world.git.push_unreachable.store(true, Ordering::SeqCst);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Unreachable, "{}", reply.text);
    assert_eq!(world.hub.holder("staging"), None);
    assert!(world.landings().is_empty());

    world.git.push_unreachable.store(false, Ordering::SeqCst);
    world.git.land_other(&[("c.txt", "theirs")]);
    let again = world.land_as("tree-01", "local_a");
    assert_eq!(again.exit, Exit::Success, "{}", again.text);
    assert_eq!(world.git.pushes().len(), 1);
    assert_eq!(world.landings().len(), 1);
    assert!(world.landing.records.all().unwrap().is_empty());
}

#[test]
fn a_push_that_landed_before_its_connection_failed_closes_the_ticket_without_a_second_push() {
    let world = world();
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    world
        .git
        .push_lands_then_fails
        .store(true, Ordering::SeqCst);

    let reply = world.land_as("tree-01", "local_a");
    assert_eq!(reply.exit, Exit::Unreachable, "{}", reply.text);
    let pushed = world.git.tip_now();
    assert!(world.landings().is_empty());

    world.git.land_other(&[("c.txt", "theirs")]);
    let again = world.land_as("tree-01", "local_a");

    assert_eq!(again.exit, Exit::Success, "{}", again.text);
    assert_eq!(world.git.pushes().len(), 1);
    let landed = world.landings();
    assert_eq!(landed.len(), 1);
    assert_eq!(landed[0].work.commits, [pushed]);
    assert!(world.landing.records.all().unwrap().is_empty());
}

#[test]
fn sync_rebases_the_branch_onto_the_tip() {
    let world = world();
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    world.git.land_other(&[("c.txt", "one")]);
    world.git.land_other(&[("c.txt", "two")]);
    let req = world.request("sync", "tree-01", "local_a", &[]);
    let mut ctx = world.ctx.clone();
    ctx.session.clone_from(&req.session);
    ctx.tree = Some(TreeId::new("tree-01").unwrap());
    let sync = SyncVerb {
        landing: world.landing.clone(),
    };

    let reply = sync.run(&ctx, &req);
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(
        reply.text.contains("2 commits landed since"),
        "{}",
        reply.text
    );
    let again = sync.run(&ctx, &req);
    assert!(again.text.starts_with("current"), "{}", again.text);
}

#[test]
fn a_refusal_that_is_not_a_moved_tip_fails_the_landing_at_once() {
    let world = world();
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    world.git.push_refused.store(true, Ordering::SeqCst);

    let reply = world.land_as("tree-01", "local_a");

    assert_ne!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(reply.text.contains("hook declined"), "{}", reply.text);
    assert_eq!(world.gate.passes.lock().unwrap().len(), 1);
    assert!(world.landings().is_empty());
}

#[test]
fn a_landing_whose_pushes_are_always_rejected_gives_up_after_ten_passes() {
    let world = world();
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    world.git.push_always_rejected.store(true, Ordering::SeqCst);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Conflict, "{}", reply.text);
    assert!(
        reply.text.contains("gave up after 10 passes"),
        "{}",
        reply.text
    );
    assert!(reply.text.contains("fetch first"), "{}", reply.text);
    // The tip never moved, so every pass after the first carries the first pass's gate over.
    assert_eq!(world.gate.passes.lock().unwrap().len(), 1);
    assert!(world.landings().is_empty());
    assert_eq!(world.hub.holder("staging"), None);
}

#[test]
fn an_empty_rebase_does_not_close_a_ticket_whose_push_never_reached_the_remote() {
    let world = world();
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    let path = PathBuf::from("/trees/tree-01");
    // The ticket's commits are on the branch already, so the rebase leaves nothing to land,
    // but the record's commit was never pushed.
    let tip = world.git.tip_now();
    world.git.check_out(&path, &tip);
    let never_pushed = Commit::new("f".repeat(40)).unwrap();
    let held = world.trees.tickets.lock().unwrap()[&TreeId::new("tree-01").unwrap()].clone();
    let record = LandingRecord {
        landed: Landed {
            ticket: held.ticket.clone(),
            machine: world.ctx.machine.clone(),
            session: held.session.clone().unwrap(),
            work: LandedWork {
                commits: vec![never_pushed.clone()],
                files: Vec::new(),
                note: "the note".to_owned(),
            },
            fresh: false,
        },
        commit: never_pushed,
        state: RecordState::Pushing,
        written_at: None,
        tree: None,
    };
    world.landing.records.write(&record).unwrap();

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Usage, "{}", reply.text);
    assert!(reply.text.contains("nothing to land"), "{}", reply.text);
    assert!(world.landings().is_empty());
    assert!(world.landing.records.all().unwrap().is_empty());
}

#[test]
fn a_malformed_record_is_moved_aside_and_the_others_are_still_delivered() {
    let world = world();
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "mine")]);
    let held = world.trees.tickets.lock().unwrap()[&TreeId::new("tree-01").unwrap()].clone();
    let commit = Commit::new("e".repeat(40)).unwrap();
    let record = LandingRecord {
        landed: Landed {
            ticket: held.ticket.clone(),
            machine: world.ctx.machine.clone(),
            session: held.session.clone().unwrap(),
            work: LandedWork {
                commits: vec![commit.clone()],
                files: Vec::new(),
                note: "the note".to_owned(),
            },
            fresh: false,
        },
        commit,
        state: RecordState::Pushed,
        written_at: None,
        tree: None,
    };
    world.landing.records.write(&record).unwrap();
    // "a.1" sorts before "t.1", so the bad file is read first.
    std::fs::write(world.records.join("a.1.json"), "{ not json").unwrap();

    let delivered = world
        .landing
        .records
        .deliver_all(world.hub.as_ref())
        .unwrap();

    assert_eq!(delivered, [held.ticket]);
    assert_eq!(world.landings().len(), 1);
    assert!(world.records.join("a.1.json.bad").exists());
    assert!(!world.records.join("a.1.json").exists());
    assert!(world.landing.records.all().unwrap().is_empty());
}

/// The ticket as the hub stores it, with `title`.
fn ticket_titled(id: &str, title: &str) -> ignite_core::protocol::tickets::TicketDetail {
    serde_json::from_value(serde_json::json!({
        "id": id, "parent": null, "next_child": 1, "title": title,
        "body": "", "type": "task", "status": "in_progress", "priority": 2,
        "assignee": null, "labels": [], "defer_until": null, "close_reason": null,
        "created_at": "2027-01-01T00:00:00.000Z", "created_by": "test",
        "updated_at": "2027-01-01T00:00:00.000Z", "closed_at": null, "version": 1,
        "session": null, "edges": [], "children": [], "ready": false, "blocked_by": [],
    }))
    .unwrap()
}

#[test]
fn a_landing_prints_the_tickets_title_in_its_title_line() {
    let world = world();
    world
        .hub
        .hub
        .insert_ticket(ticket_titled("t.1", "Fix the thing"));
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "a by t.1")]);

    let reply = world.land_as("tree-01", "local_a");

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    let title = reply
        .text
        .lines()
        .find(|l| l.trim_start().starts_with("title"))
        .unwrap();
    assert!(title.ends_with("✅ [t.1] Fix the thing"), "{title}");
}

#[test]
fn a_landing_with_the_hub_unable_to_name_the_ticket_prints_only_its_id() {
    let world = world();
    world.hold("tree-01", "t.1", "local_a", &[("a.txt", "a by t.1")]);

    let reply = world.land_as("tree-01", "local_a");

    let title = reply
        .text
        .lines()
        .find(|l| l.trim_start().starts_with("title"))
        .unwrap();
    assert!(title.ends_with("✅ [t.1]"), "{title}");
}
