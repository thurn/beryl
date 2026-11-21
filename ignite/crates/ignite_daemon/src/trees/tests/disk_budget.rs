//! The disk sweep recalculates its budget from a volume sample and the latest fleet notice.

use ignite_core::capacity::BYTES_PER_GB;
use ignite_core::fleet::FleetNotice;
use ignite_os::system::DiskSpace;

use super::*;

fn sample(total_gb: u64, available: u64) -> DiskSpace {
    DiskSpace {
        total: total_gb * BYTES_PER_GB,
        available,
    }
}

fn write_build(trees: &Trees, tree: &str) {
    write(
        &trees.config().path(&id(tree)),
        "out/build",
        &"x".repeat(200_000),
    );
}

#[test]
fn outside_growth_reduces_the_budget_and_the_sweep_brings_trees_under_it() {
    let f = Fixture::new(2, 1);
    for tree in ["tree-01", "tree-02"] {
        write_build(&f.trees, tree);
    }

    let reserve = 50 * BYTES_PER_GB;
    let roomy = f
        .trees
        .hold_disk_with(|| Some(sample(128, reserve + BYTES_PER_GB)))
        .unwrap();
    assert_eq!(roomy.limit, Some(roomy.before + BYTES_PER_GB));
    assert_eq!(roomy.limit_source, Some(DiskLimitSource::Disk));
    assert!(roomy.cleared.is_empty());

    let tight = f
        .trees
        .hold_disk_with(|| Some(sample(128, reserve - 1)))
        .unwrap();
    assert_eq!(tight.limit, Some(tight.before - 1));
    assert_eq!(tight.limit_source, Some(DiskLimitSource::Disk));
    assert_eq!(tight.cleared.len(), 1);
    assert!(tight.after <= tight.limit.unwrap(), "{tight:?}");
}

#[test]
fn the_latest_notice_sets_or_clears_the_fleet_cap() {
    let f = Fixture::new(1, 1);
    let db = f.scratch.0.join("daemon.db");
    let pool = crate::pool::Pool::new(ignite_store::local::open(&db).unwrap());
    let trees = Trees::new(
        f.trees.config().clone(),
        ignite_store::local::open(&db).unwrap(),
        Arc::new(FakeClock::new(
            Timestamp::parse("2026-11-18T00:00:00.000Z").unwrap(),
        )),
        f.builds.clone(),
    );
    trees.setup(&f.tip()).unwrap();
    write_build(&trees, "tree-01");
    let disk = || Some(sample(926, 200 * BYTES_PER_GB));
    let before_notice = trees.hold_disk_with(disk).unwrap();
    assert_eq!(
        before_notice.limit,
        Some(107_400_000_000 + before_notice.before)
    );
    assert_eq!(before_notice.limit_source, Some(DiskLimitSource::Disk));
    assert!(before_notice.cleared.is_empty());

    let mut notice = FleetNotice {
        disk_cap_gb: Some(0),
        ..FleetNotice::default()
    };
    crate::fleet_notice::store(&pool, &notice).unwrap();
    let capped = trees.hold_disk_with(disk).unwrap();
    assert_eq!(capped.limit, Some(0));
    assert_eq!(capped.limit_source, Some(DiskLimitSource::Cap));
    assert_eq!(capped.cleared, [id("tree-01")]);
    assert_eq!(capped.after, 0);

    notice.disk_cap_gb = None;
    crate::fleet_notice::store(&pool, &notice).unwrap();
    write_build(&trees, "tree-01");
    let automatic = trees.hold_disk_with(disk).unwrap();
    assert_eq!(automatic.limit, Some(107_400_000_000 + automatic.before));
    assert_eq!(automatic.limit_source, Some(DiskLimitSource::Disk));
    assert!(automatic.cleared.is_empty());
}
