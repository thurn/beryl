//! `/tips/unclaimed`: which of a family's checks at a tip still need a run, asked before a
//! tip round builds the family.

use serde_json::{Value, json};

use super::{TestHub, key, new_result};

fn ask(machine: &str, batches: &[(u32, &[&str])]) -> Value {
    let batches: Vec<Value> = batches
        .iter()
        .enumerate()
        .map(|(n, (batch, checks))| {
            let keys: Vec<Value> = checks.iter().map(|c| key(c, n, &Value::Null)).collect();
            json!({"batch": batch, "checks": keys})
        })
        .collect();
    json!({"commit": "abcd", "family": "rust", "machine": machine, "batches": batches})
}

fn claim(machine: &str, batch: u32) -> Value {
    json!({"commit": "abcd", "family": "rust", "batch": batch, "machine": machine})
}

#[test]
fn a_family_held_or_answered_elsewhere_has_no_unclaimed_checks() {
    let hub = TestHub::start("unclaimed");
    let batches: &[(u32, &[&str])] = &[(0, &["rust:a", "rust:b"]), (1, &["rust:c"])];
    // Nobody holds anything yet: every check still needs a run.
    let reply = hub.post("/tips/unclaimed", &ask("m-2", batches));
    assert_eq!(reply["checks"], json!(["rust:a", "rust:b", "rust:c"]));

    // m-1 holds batch 0 and has stored the result of the check in batch 1.
    assert_eq!(hub.post("/tips/claim", &claim("m-1", 0))["granted"], true);
    hub.post(
        "/results/store",
        &json!({"results": [new_result("rust:c", 1, "pass", 1)]}),
    );
    let reply = hub.post("/tips/unclaimed", &ask("m-2", batches));
    assert_eq!(reply["checks"], json!([]));

    // A machine's own claim does not hold a batch against it.
    let reply = hub.post("/tips/unclaimed", &ask("m-1", batches));
    assert_eq!(reply["checks"], json!(["rust:a", "rust:b"]));
}

#[test]
fn an_expired_claim_leaves_its_checks_unclaimed() {
    let hub = TestHub::start("unclaimed-expired");
    // The test hub answers at 100 seconds past the epoch; this claim expired at 50.
    hub.conn
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO tip_claims (commit_sha, family, batch, machine, expires_at)
             VALUES ('abcd', 'rust', 0, 'm-1', ?1)",
            [super::at(50.0).to_string()],
        )
        .unwrap();
    // Asking needs no token: the route only reads.
    let (status, reply) = hub.send(
        "POST",
        "/tips/unclaimed",
        Some(&ask("m-2", &[(0, &["rust:a"])])),
        false,
    );
    assert_eq!(status, 200, "{reply}");
    assert_eq!(reply["checks"], json!(["rust:a"]));
}
