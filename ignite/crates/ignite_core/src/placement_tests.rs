use super::*;
use crate::dispatch::{SavedWork, TicketKind};

fn ready() -> ReadyTicket {
    ReadyTicket {
        id: "ticket".parse().unwrap(),
        kind: TicketKind::Ordinary,
        holds_tip: false,
        saved_work: SavedWork::None,
        epic: crate::shares::Group::General,
        allocation: 100,
        chain: 0,
        created_at: Timestamp::EPOCH,
        labels: Vec::new(),
        expected_cores: 2.0,
        needs_build_slot: false,
        families: vec![crate::dispatch::RequiredFamily {
            name: "native".into(),
            platforms: crate::manifest::Platforms::Any(crate::manifest::AnyPlatform::Any),
        }],
        files: vec!["engine/src/main.rs".into(), "client/src/main.rs".into()],
        saving: None,
        pins: Pins::default(),
        needs_os: None,
    }
}

fn ticket(ready: &ReadyTicket) -> PlacementTicket<'_> {
    PlacementTicket {
        ready,
        pins: &ready.pins,
    }
}

fn machine<'a>(name: &'a str, capacity: &'a CapacityReport) -> MachineView<'a> {
    MachineView {
        name,
        os: Os::Macos,
        families: &[],
        state: MachineState::Active,
        capacity,
        provider: Some("codex"),
    }
}

#[test]
fn cost_uses_donors_from_free_and_leased_trees_and_at_least_one_core() {
    let ready = ready();
    let ticket = ticket(&ready);
    let mut report = CapacityReport {
        free_cores: 5.0,
        worker_room: 1,
        queue_seconds: 10.0,
        outputs: ["engine/target", "client/target", "tools/target"]
            .map(str::to_owned)
            .into(),
        ..CapacityReport::default()
    };
    assert_eq!(cost(&ticket, &machine("one", &report)), Some(410.0));
    report.free_trees.push(vec!["engine/target".into()]);
    report.free_cores = 0.5;
    assert_eq!(cost(&ticket, &machine("one", &report)), Some(1010.0));
    report
        .leased_trees
        .insert("other".parse().unwrap(), vec!["client/target".into()]);
    assert_eq!(cost(&ticket, &machine("one", &report)), Some(10.0));
}

#[test]
fn ineligible_machines_have_no_cost() {
    let mut ready = ready();
    ready.needs_os = Some(Os::Macos);
    ready.pins = Pins {
        machine: Some("one".into()),
        provider: Some("codex".into()),
    };
    let ticket = ticket(&ready);
    let room = CapacityReport {
        worker_room: 1,
        ..CapacityReport::default()
    };
    let no_room = CapacityReport::default();
    let wrong_families = ["web".to_owned()];
    for view in [
        MachineView {
            state: MachineState::ChecksOnly,
            ..machine("one", &room)
        },
        MachineView {
            os: Os::Windows,
            ..machine("one", &room)
        },
        MachineView {
            families: &wrong_families,
            ..machine("one", &room)
        },
        MachineView {
            provider: None,
            ..machine("one", &room)
        },
        MachineView {
            provider: Some("claude"),
            ..machine("one", &room)
        },
        machine("other", &room),
        machine("one", &no_room),
    ] {
        assert_eq!(cost(&ticket, &view), None);
    }
    assert_eq!(cost(&ticket, &machine("one", &room)), Some(0.0));
}

#[test]
fn only_large_time_differences_or_exact_ties_defer_and_patience_expires() {
    let ready = ready();
    let ticket = ticket(&ready);
    let fast = CapacityReport {
        worker_room: 1,
        free_cores: 3.0,
        admit_cores: 4.0,
        ..CapacityReport::default()
    };
    let slower = CapacityReport {
        queue_seconds: 31.0,
        free_cores: 5.0,
        admit_cores: 10.0,
        ..fast.clone()
    };
    let almost = CapacityReport {
        queue_seconds: 30.0,
        ..slower.clone()
    };
    let tied = CapacityReport {
        queue_seconds: 0.0,
        ..slower.clone()
    };
    let first = Timestamp::EPOCH;
    let peer = || [machine("b", &fast)];
    assert_eq!(
        takes(
            &ticket,
            &machine("a", &slower),
            &peer(),
            Some(first),
            first.plus_seconds(59.999)
        ),
        Some("b")
    );
    assert_eq!(
        takes(
            &ticket,
            &machine("a", &slower),
            &peer(),
            Some(first),
            first.plus_seconds(60.0)
        ),
        None
    );
    assert_eq!(
        takes(&ticket, &machine("a", &almost), &peer(), None, first),
        None
    );
    assert_eq!(
        takes(&ticket, &machine("a", &tied), &peer(), None, first),
        Some("b")
    );
    assert_eq!(
        takes(&ticket, &machine("a", &fast), &peer(), None, first),
        None
    );
    assert_eq!(
        takes(&ticket, &machine("c", &fast), &peer(), None, first),
        Some("b")
    );
}
