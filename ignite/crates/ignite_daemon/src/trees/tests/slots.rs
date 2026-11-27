use std::sync::mpsc;

use ignite_core::capacity::BYTES_PER_GB;
use ignite_core::fleet::{Auto, FleetNotice, TreeSettings, TreeSpace};
use ignite_os::system::DiskSpace;

use super::*;

#[test]
fn slots_use_the_configured_space_or_the_tree_volume_and_round_down() {
    let volume = Some(DiskSpace {
        total: 399 * BYTES_PER_GB + 4,
        available: 15,
    });
    let mut settings = TreeSettings {
        tree_ceiling_gb: 100,
        tree_space_gb: Some(TreeSpace::Automatic(Auto::Auto)),
        ..TreeSettings::default()
    };
    assert_eq!(slot_count(&settings, volume), 3);
    settings.tree_space_gb = Some(TreeSpace::Gigabytes(205));
    assert_eq!(slot_count(&settings, volume), 2);
    settings.tree_space_gb = Some(TreeSpace::Gigabytes(30));
    assert_eq!(slot_count(&settings, volume), 1);
}

fn set_slot_budget(f: &Fixture, slots: u32) {
    crate::fleet_notice::write(
        &f.trees.lock().db,
        &FleetNotice {
            trees: TreeSettings {
                tree_ceiling_gb: 100,
                tree_space_gb: Some(TreeSpace::Gigabytes(100 * slots)),
                ..TreeSettings::default()
            },
            ..FleetNotice::default()
        },
    )
    .unwrap();
}

#[test]
fn a_cold_tip_build_gets_a_slot_when_ticket_builds_are_at_their_limit() {
    let f = Fixture::with_landing(3, 2, 1);
    set_slot_budget(&f, 3);
    let first = f.lease("T.1", false);
    let second = f.lease("T.2", false);
    let third = f.lease("T.3", false);
    assert_eq!(f.trees.build_slots().free, 3);
    assert_eq!(
        f.trees.dispatch_pressure(0, 0, None).build_slots_free,
        2,
        "empty leases do not occupy a slot"
    );
    let first_slot = f
        .trees
        .take_build_slot(&first.tree, &[], true)
        .unwrap()
        .unwrap();
    write(&first.path, "out/compiled", "first");
    drop(first_slot);
    let guard = f
        .trees
        .take_build_slot(&second.tree, &[], true)
        .unwrap()
        .unwrap();
    let pressure = f.trees.dispatch_pressure(0, 0, None);
    assert_eq!(pressure.build_slots_free, 0);
    assert_eq!(
        pressure.slot_holders,
        ["T.1".parse().unwrap(), "T.2".parse().unwrap()].into()
    );
    assert!(
        f.trees
            .take_build_slot(&third.tree, &[], true)
            .unwrap()
            .is_none()
    );
    assert!(
        f.trees
            .take_build_slot(&third.tree, &[], false)
            .unwrap()
            .is_none(),
        "seeding uses the same ordinary limit"
    );

    let tip = f
        .trees
        .lease_landing("tip verification", &f.tip())
        .unwrap()
        .unwrap();
    assert!(!tip.path.join("out").exists());
    let tip_slot = f
        .trees
        .take_build_slot(&tip.tree, &[], true)
        .unwrap()
        .expect("the cold tip build can take the reserved slot");
    write(&tip.path, "out/compiled", "tip");
    assert_eq!(f.trees.build_directories().unwrap().len(), 3);
    let other = f
        .trees
        .lease_landing("another tip family", &f.tip())
        .unwrap()
        .unwrap();
    assert!(
        f.trees
            .take_build_slot(&other.tree, &[], true)
            .unwrap()
            .is_none(),
        "tip builds also respect the volume limit"
    );
    assert_eq!(
        fs::read_to_string(first.path.join("out/compiled")).unwrap(),
        "first"
    );
    assert!(
        f.trees
            .take_build_slot(&first.tree, &[], true)
            .unwrap()
            .is_some(),
        "a ticket can keep building in the slot it already occupies"
    );
    drop(guard);

    // A build that wrote no output returns its reservation even while tip holds its slot.
    let pressure = f.trees.dispatch_pressure(0, 0, None);
    assert_eq!(pressure.build_slots_free, 1);
    assert_eq!(pressure.slot_holders, ["T.1".parse().unwrap()].into());
    assert!(
        f.trees
            .take_build_slot(&third.tree, &[], true)
            .unwrap()
            .is_some()
    );
    drop(tip_slot);
}

#[test]
fn a_one_slot_machine_shares_the_slot_between_ticket_and_tip_builds() {
    let f = Fixture::new(1, 1);
    set_slot_budget(&f, 1);
    let ticket = f.lease("T.1", false);
    let tip = f
        .trees
        .lease_landing("tip verification", &f.tip())
        .unwrap()
        .unwrap();
    let slot = f
        .trees
        .take_build_slot(&ticket.tree, &[], true)
        .unwrap()
        .expect("ticket builds still run on a one-slot machine");
    write(&ticket.path, "out/compiled", "ticket");
    drop(slot);
    assert!(
        f.trees
            .take_build_slot(&tip.tree, &[], true)
            .unwrap()
            .is_none()
    );
    assert!(ticket.path.join("out/compiled").is_file());
    store::release(
        &f.trees.lock().db,
        ticket.tree.as_str(),
        Some(f.tip().as_str()),
        &f.trees.now(),
    )
    .unwrap();
    let _tip_slot = f
        .trees
        .take_build_slot(&tip.tree, &[], true)
        .unwrap()
        .unwrap();
    assert_eq!(f.trees.build_directories().unwrap().len(), 1);
    assert!(!ticket.path.join("out").exists());
}

#[test]
fn idle_work_keeps_the_warm_reserved_slot_available_to_tip() {
    let f = Fixture::with_landing(1, 2, 1);
    set_slot_budget(&f, 2);
    let ticket = f.lease("T.1", false);
    write(&ticket.path, "out/compiled", "ticket");
    let warm = f.trees.lease_landing("tip", &f.tip()).unwrap().unwrap();
    write(&warm.path, "out/compiled", "tip");
    f.trees.release_landing(&warm.tree, Some(&f.tip())).unwrap();

    let idle = f
        .trees
        .lease_spare_landing(crate::idle::LANDING_HOLDER, &f.tip())
        .unwrap()
        .unwrap();
    assert_ne!(idle.tree, warm.tree);
    assert!(
        f.trees
            .take_build_slot(&idle.tree, &[], true)
            .unwrap()
            .is_none()
    );
    assert!(warm.path.join("out/compiled").is_file());
    assert_eq!(
        f.trees
            .lease_landing("tip", &f.tip())
            .unwrap()
            .unwrap()
            .tree,
        warm.tree
    );
}

#[test]
fn admission_clears_the_oldest_free_tree_and_prefers_an_empty_slot() {
    let f = Fixture::new(4, 1);
    let leases: Vec<_> = (1..=4)
        .map(|i| {
            f.clock.advance(1.0);
            f.lease(&format!("T.{i}"), false)
        })
        .collect();
    for lease in &leases[..3] {
        write(&lease.path, "out/compiled", "build");
        f.clock.advance(1.0);
        store::release(
            &f.trees.lock().db,
            lease.tree.as_str(),
            Some(f.tip().as_str()),
            &f.trees.now(),
        )
        .unwrap();
    }
    let cold = &leases[3].tree;
    let empty = f.trees.take_slot(cold, &[], true, 5).unwrap().unwrap();
    assert!(
        leases[..3]
            .iter()
            .all(|lease| lease.path.join("out/compiled").is_file())
    );
    drop(empty);

    assert!(f.trees.take_slot(cold, &[], true, 4).unwrap().is_some());
    let oldest = &leases[0];
    assert!(!oldest.path.join("out").exists());
    assert_eq!(f.trees.row(&oldest.tree).unwrap().built_commit, None);
    assert!(
        leases[1..3]
            .iter()
            .all(|lease| lease.path.join("out/compiled").is_file())
    );
}

#[test]
fn an_over_ceiling_tree_keeps_its_outputs_while_a_command_is_using_them() {
    let f = Fixture::new(1, 1);
    let lease = f.lease("T.1", false);
    let output = lease.path.join("out/compiled");
    write(&lease.path, "out/compiled", "over the ceiling");
    let command = command_lock(&lease.path).unwrap();
    command.lock_shared().unwrap();
    let (waiting, reached) = mpsc::sync_channel(0);
    let trees = &f.trees;
    let task = std::thread::scope(|scope| {
        let build = scope.spawn(move || {
            let waiting = std::cell::RefCell::new(Some(waiting));
            let attempts = std::cell::Cell::new(0);
            trees.prepare_build(&lease.tree, 10, &|| {
                // Notify after at least one failed attempt to take the lock.
                attempts.set(attempts.get() + 1);
                if attempts.get() == 2
                    && let Some(waiting) = waiting.borrow_mut().take()
                {
                    let _ = waiting.send(());
                }
                false
            })
        });
        let observed_wait = reached.recv_timeout(Duration::from_secs(15));
        let kept = output.is_file();
        drop(reached);
        drop(command);
        let result = build.join().unwrap();
        observed_wait.unwrap();
        assert!(kept);
        result
    });
    task.unwrap();
    assert!(!output.exists());
    assert!(f.trees.last_disk_cleanup_at().is_some());
    assert!(
        f.trees
            .last_build_cleanup()
            .unwrap()
            .contains("build directories")
    );
}

#[cfg(target_os = "macos")]
#[test]
fn a_command_started_directly_in_the_tree_keeps_its_build_outputs() {
    use std::io::{BufRead as _, BufReader};
    use std::process::Stdio;

    let f = Fixture::new(1, 1);
    let lease = f.lease("T.1", false);
    let output = lease.path.join("out/compiled");
    write(&lease.path, "out/compiled", "over the ceiling");
    let mut command = Command::new("/bin/sh")
        .args(["-c", "printf 'ready\n'; read line"])
        .current_dir(&lease.path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut ready = String::new();
    BufReader::new(command.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert_eq!(ready.trim(), "ready");
    let steps = crate::steps::Steps::new(f.clock.clone());
    let idle = steps.begin(
        crate::steps::Kind::Verb("idle-job".to_owned()),
        crate::steps::Work::default(),
    );
    let (waiting, reached) = mpsc::sync_channel(1);
    std::thread::scope(|scope| {
        let trees = &f.trees;
        let activity = &idle;
        let build = scope.spawn(move || {
            let _entered = activity.enter();
            let attempts = std::cell::Cell::new(0);
            trees.prepare_build(&lease.tree, 10, &|| {
                attempts.set(attempts.get() + 1);
                if attempts.get() == 2 {
                    let _ = waiting.send(());
                }
                false
            })
        });
        let observed_wait = reached.recv_timeout(Duration::from_secs(15));
        let kept = output.is_file();
        let owner = idle.view().waiting_for;
        let pid = command.id();
        drop(command.stdin.take());
        command.wait().unwrap();
        let result = build.join().unwrap();
        observed_wait.unwrap();
        assert!(kept);
        assert!(owner.unwrap().contains(&format!("PID {pid}")));
        result.unwrap();
    });
    assert!(!output.exists());
}

#[test]
fn admission_evicts_free_ticket_trees_before_older_landing_trees() {
    let f = Fixture::with_landing(2, 2, 1);
    set_slot_budget(&f, 3);

    // Populate both landing trees and release landing-1 first so its last_used is older than
    // the ticket tree's, while landing-2 stays leased so all three slots are occupied.
    f.clock.advance(1.0);
    let landing = f.trees.lease_landing("tip", &f.tip()).unwrap().unwrap();
    write(&landing.path, "out/compiled", "landing-1");
    let landing2 = f.trees.lease_landing("tip-2", &f.tip()).unwrap().unwrap();
    write(&landing2.path, "out/compiled", "landing-2");
    assert_ne!(landing.tree, landing2.tree);
    f.trees
        .release_landing(&landing.tree, Some(&f.tip()))
        .unwrap();

    f.clock.advance(10.0);
    let ticket = f.lease("T.1", false);
    write(&ticket.path, "out/compiled", "ticket-1");
    let newcomer = f.lease("T.2", false);
    assert_ne!(ticket.tree, newcomer.tree);
    store::release(
        &f.trees.lock().db,
        ticket.tree.as_str(),
        Some(f.tip().as_str()),
        &f.trees.now(),
    )
    .unwrap();

    // A new ticket tree needs a slot while ordinary_count (1) < ordinary_limit (2) and must
    // evict the free ticket tree, keeping landing-1 warm.
    f.clock.advance(10.0);
    let _slot = f
        .trees
        .take_build_slot(&newcomer.tree, &[], true)
        .unwrap()
        .expect("free ticket tree should be reclaimed");
    assert!(!ticket.path.join("out").exists());
    assert!(
        landing.path.join("out/compiled").is_file(),
        "warm landing tree should stay intact when a free ticket tree can be reclaimed"
    );
}

#[test]
fn an_unheld_leased_ticket_tree_is_reclaimed_for_a_build_slot() {
    let f = Fixture::with_landing(3, 1, 1);
    set_slot_budget(&f, 3);

    // Fill the two ordinary slots: one real lease and one leaked `leased` row with no ticket
    // and no session, both with build outputs on disk.
    let active = f.lease("T.1", false);
    write(&active.path, "out/compiled", "active");
    let leaked_id = id("tree-02");
    let mut leaked = f.trees.row(&leaked_id).unwrap();
    write(Path::new(&leaked.path), "out/compiled", "stale");
    leaked.state = store::State::Leased.as_str().to_owned();
    leaked.ticket = None;
    leaked.session = None;
    leaked.branch = None;
    leaked.leased_at = Some(f.trees.now());
    store::upsert(&f.trees.lock().db, &leaked).unwrap();

    let newcomer = f.lease("T.2", false);
    assert_ne!(newcomer.tree, active.tree);
    assert_ne!(newcomer.tree, leaked_id);
    assert_eq!(
        f.trees.dispatch_pressure(0, 0, None).build_slots_free,
        1,
        "an unheld leased ticket tree counts as reclaimable"
    );

    let _slot = f
        .trees
        .take_build_slot(&newcomer.tree, &[], true)
        .unwrap()
        .expect("unheld leased ticket tree should be reclaimed for a build slot");
    assert!(!Path::new(&leaked.path).join("out").exists());
    let healed = f.trees.row(&leaked_id).unwrap();
    assert_eq!(healed.state, "free");
    assert_eq!(healed.leased_at, None);
    assert!(active.path.join("out/compiled").is_file());
}
