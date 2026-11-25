//! The tip routes a round asks before it builds a family.

use ignite_core::ids::{Commit, InputHash, MachineId};
use ignite_core::protocol::hub::{ResultKey, TipUnclaimed, TipUnclaimedBatch};
use ignite_core::traits::Hub;

use super::FakeHttpHub;

fn ask() -> TipUnclaimed {
    TipUnclaimed {
        commit: Commit::new("abcd").unwrap(),
        family: "unit@windows".to_owned(),
        machine: MachineId::new("m-1").unwrap(),
        batches: vec![TipUnclaimedBatch {
            batch: 3,
            checks: vec![ResultKey {
                check: "unit:a".parse().unwrap(),
                hash: InputHash::new("a".repeat(64)).unwrap(),
                platform: None,
            }],
        }],
    }
}

#[test]
fn unclaimed_checks_are_asked_of_their_route() {
    let hub = FakeHttpHub::start(|_, _, _| (200, r#"{"checks": ["unit:a"]}"#.to_owned()));
    let reply = hub.client().tip_unclaimed(&ask()).unwrap();
    assert_eq!(reply.checks, vec!["unit:a".parse().unwrap()]);
    let seen = hub.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(
        seen[0].method.clone() + " " + &seen[0].url,
        "POST /test/api/tips/unclaimed"
    );
    let sent: TipUnclaimed = serde_json::from_str(&seen[0].body).unwrap();
    assert_eq!(sent, ask());
}

/// A hub from before the route answers 404, which the round reads as an error and builds the
/// family as before.
#[test]
fn a_hub_without_the_route_is_an_error() {
    let hub = FakeHttpHub::start(|_, _, _| (404, r#"{"error": "no route"}"#.to_owned()));
    assert!(hub.client().tip_unclaimed(&ask()).is_err());
}
