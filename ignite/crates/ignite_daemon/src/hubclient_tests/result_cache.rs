use ignite_core::config::Os;
use ignite_core::ids::{InputHash, MachineId, RecordId};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{CachedResult, LookupReply, LookupRequest, ResultKey};
use ignite_core::time::Timestamp;
use ignite_core::traits::Hub;

use super::FakeHttpHub;

#[test]
fn large_result_cache_replies_preserve_hits_and_missing_positions() {
    let hash = InputHash::new("a".repeat(64)).unwrap();
    let machine = MachineId::new("m-1").unwrap();
    let keys: Vec<_> = (0..60_000)
        .map(|index| ResultKey {
            check: format!("native:case-{index}").parse().unwrap(),
            hash: hash.clone(),
            platform: Some(if index % 2 == 0 {
                Os::Macos
            } else {
                Os::Windows
            }),
        })
        .collect();
    let expected = LookupReply {
        results: keys
            .iter()
            .enumerate()
            .map(|(index, key)| {
                (index != 30_000).then(|| {
                    let failed = index % 2 == 1;
                    CachedResult {
                        key: key.clone(),
                        outcome: if failed { Outcome::Fail } else { Outcome::Pass },
                        machine: machine.clone(),
                        record: RecordId(index as u64),
                        at: Timestamp::EPOCH,
                        output: failed.then(|| format!("out/{index}.log")),
                    }
                })
            })
            .collect(),
    };
    let body = serde_json::to_string(&expected).unwrap();
    assert!(body.len() > 10 * 1024 * 1024);

    let hub = FakeHttpHub::start(move |_, _, _| (200, body.clone()));
    let found = hub.client().lookup(&keys).unwrap();
    assert_eq!(found.len(), keys.len());
    for (index, (actual, expected)) in found.iter().zip(&expected.results).enumerate() {
        assert_eq!(actual, expected, "result at request index {index}");
    }
    let seen = hub.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].url, "/test/api/results/lookup");
    let sent: LookupRequest = serde_json::from_str(&seen[0].body).unwrap();
    assert!(sent.keys == keys, "the request keys must be in order");
}
