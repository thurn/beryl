use super::*;

#[test]
fn capacities_for_the_three_machine_profiles() {
    for (os, physical, logical, express, tip_build, batch, landing, ceiling, page_slots) in [
        (Os::Macos, 18, 18, 3, 4, 6, 3, 36, 9),
        (Os::Macos, 10, 10, 2, 2, 3, 2, 20, 5),
        (Os::Windows, 12, 20, 2, 2, 4, 2, 40, 10),
    ] {
        let facts = MachineFacts {
            os,
            logical_cores: logical,
            physical_cores: physical,
            memory_mb: 0,
            disk_total_gb: 0,
        };
        assert_eq!(
            derive(&facts),
            Derived {
                express_cores: express,
                tip_build_cores: tip_build,
                floor: Floor { batch, tip: 2 },
                landing_trees: landing,
                worker_ceiling: ceiling,
            },
            "{facts:?}",
        );
        // A machine's check families determine whether it uses these slots.
        assert_eq!(resource_slots(logical, 0.5), page_slots);
    }
}

#[test]
fn capacity_rounding_and_bounds() {
    let mut facts = MachineFacts {
        os: Os::Macos,
        logical_cores: 1,
        physical_cores: 1,
        memory_mb: 0,
        disk_total_gb: 0,
    };
    let small = derive(&facts);
    assert_eq!(
        (
            small.express_cores,
            small.tip_build_cores,
            small.floor.batch
        ),
        (2, 2, 2)
    );
    facts.physical_cores = 15;
    assert_eq!(derive(&facts).express_cores, 3);
    facts.physical_cores = 48;
    let large = derive(&facts);
    assert_eq!((large.express_cores, large.tip_build_cores), (4, 6));
    assert_eq!(resource_slots(1, 0.5), 1);
    assert_eq!(resource_slots(11, 0.5), 5);
}

/// A machine that declares `tip_build_cores` keeps that many, whatever its cores would give;
/// one that declares nothing keeps the derived count.
#[test]
fn a_declared_tip_reserve_replaces_the_derived_one() {
    let facts = MachineFacts {
        os: Os::Windows,
        logical_cores: 20,
        physical_cores: 12,
        memory_mb: 0,
        disk_total_gb: 0,
    };
    let declared = Machine {
        tip_build_cores: Some(6),
        ..Machine::default()
    };
    assert_eq!(derive_for(&facts, &declared).tip_build_cores, 6);
    assert_eq!(derive_for(&facts, &Machine::default()), derive(&facts));
}
