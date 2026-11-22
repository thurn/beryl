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

#[test]
fn a_cold_tree_waits_for_a_slot_when_the_built_trees_are_leased() {
    let f = Fixture::new(3, 1);
    crate::fleet_notice::write(
        &f.trees.lock().db,
        &FleetNotice {
            trees: TreeSettings {
                tree_ceiling_gb: 100,
                tree_space_gb: Some(TreeSpace::Gigabytes(200)),
                ..TreeSettings::default()
            },
            ..FleetNotice::default()
        },
    )
    .unwrap();
    let first = f.lease("T.1", false);
    let second = f.lease("T.2", false);
    let third = f.lease("T.3", false);
    assert_eq!(
        f.trees.build_slots().free,
        2,
        "empty leases do not occupy a slot"
    );
    write(&first.path, "out/compiled", "first");
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
    assert_eq!(
        fs::read_to_string(first.path.join("out/compiled")).unwrap(),
        "first"
    );
    drop(guard);

    // A build that wrote no output returns its reservation.
    let pressure = f.trees.dispatch_pressure(0, 0, None);
    assert_eq!(pressure.build_slots_free, 1);
    assert_eq!(pressure.slot_holders, ["T.1".parse().unwrap()].into());
    assert!(
        f.trees
            .take_build_slot(&third.tree, &[], true)
            .unwrap()
            .is_some()
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
    let empty = f.trees.take_slot(cold, &[], true, 4).unwrap().unwrap();
    assert!(
        leases[..3]
            .iter()
            .all(|lease| lease.path.join("out/compiled").is_file())
    );
    drop(empty);

    assert!(f.trees.take_slot(cold, &[], true, 3).unwrap().is_some());
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
    let (waiting, reached) = mpsc::sync_channel(1);
    std::thread::scope(|scope| {
        let trees = &f.trees;
        let build = scope.spawn(move || {
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
        drop(command.stdin.take());
        command.wait().unwrap();
        let result = build.join().unwrap();
        observed_wait.unwrap();
        assert!(kept);
        result.unwrap();
    });
    assert!(!output.exists());
}
