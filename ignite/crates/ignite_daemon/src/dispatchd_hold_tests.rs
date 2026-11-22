//! Fleet notices drive reporting. The hub applies model holds to the providers it is offered.

use ignite_core::fleet::FleetNotice;
use ignite_core::plan_usage::Hold;
use ignite_core::time::Timestamp;
use ignite_core::traits::Clock;

use super::cli_tests::{cli, tick};
use super::tests::{Rig, START, rig};
use crate::doctor::StatusSection;
use crate::fleet_notice;

fn status(rig: &Rig) -> Vec<String> {
    crate::pool::PauseSection {
        headless_dispatch: rig.headless_dispatch.clone(),
        hub: rig.hub.clone(),
        clock: rig.clock.clone(),
    }
    .lines()
}

#[test]
fn a_fleet_hold_is_reported_but_does_not_filter_workers_at_the_daemon() {
    let rig = rig(&[]);
    let _fake = cli(&rig, 2);
    let pool = rig.headless_dispatch.pool();
    let mut notice = FleetNotice::default();
    notice.holds.insert(
        "claude".to_owned(),
        vec![Hold {
            label: "Weekly · Opus".to_owned(),
            percent_used: 99.5,
            resets_at: Some(
                Timestamp::from_millis(START)
                    .plus_seconds(3600.0)
                    .to_string(),
            ),
            family: Some("opus".to_owned()),
        }],
    );
    fleet_notice::store(pool, &notice).unwrap();
    tick(&rig);
    let free = &rig.hub.asked()[0];
    assert!(!free.master_only);
    assert!(free.idle_workers.is_empty());
    let providers = &free.room.as_ref().unwrap().providers;
    assert!(!providers.is_empty());
    for provider in providers {
        assert!(provider.tiers.iter().any(|tier| tier == "opus"));
        assert!(provider.model_ids["opus"].contains("opus"));
    }

    let expected = "usage hold: claude · Weekly · Opus";
    assert!(status(&rig).iter().any(|line| line.starts_with(expected)));
    assert!(rig.pool(&["usage"], None).text.starts_with(expected));
    assert!(rig.pool(&["list"], None).text.contains(expected));

    fleet_notice::store(pool, &FleetNotice::default()).unwrap();
    assert!(super::usage_holds(pool, rig.clock.now()).is_empty());
    fleet_notice::store(pool, &notice).unwrap();
    rig.clock.advance(3600.0);
    assert!(super::usage_holds(pool, rig.clock.now()).is_empty());
}
