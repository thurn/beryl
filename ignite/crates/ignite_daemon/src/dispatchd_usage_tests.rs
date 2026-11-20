use ignite_core::protocol::client::DispatchAction;
use ignite_core::time::Timestamp;

use super::tests::{Rig, START, handoff_to, rig_sampling};
use super::usage::{SOURCE, USAGE_BEFORE_HANDOFF_S, USAGE_EVERY_S};
use super::*;

const OK_PLAN: &str = r#"{"plan": {"status": "ok", "windows": [
    {"label": "Weekly · all models", "percentUsed": 42.5, "resetsAt": "2026-11-24T08:00:00Z"},
    {"label": "5-hour", "percentUsed": 7, "resetsAt": "2026-11-20T15:00:00Z"}]}}"#;

/// A dispatcher that asks for samples, with `local_dispatcher` registered as its Claude
/// dispatcher and `workers` in its pool.
fn sampling(workers: &[&str]) -> Rig {
    let rig = rig_sampling(
        &[],
        ignite_store::local::open_in_memory().unwrap(),
        Timestamp::from_millis(START),
        true,
    );
    let reply = rig.pool(
        &["add", "local_dispatcher", "--app", "claude", "--dispatcher"],
        Some("local_dispatcher"),
    );
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    if !workers.is_empty() {
        let mut args = vec!["add"];
        args.extend_from_slice(workers);
        args.extend_from_slice(&["--app", "claude"]);
        let reply = rig.pool(&args, Some("local_dispatcher"));
        assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    }
    rig
}

/// Takes the next instruction, which must be a `usage`, and acknowledges it with `raw`.
fn answer_usage(rig: &Rig, raw: &str) -> Reply {
    let usage = rig.step().expect("a usage instruction");
    assert_eq!(usage.action, DispatchAction::Usage);
    assert_eq!(usage.session, None);
    assert!(
        usage
            .prompt
            .as_deref()
            .unwrap()
            .contains("select:mcp__ccd_session_mgmt__get_usage")
    );
    let reply = rig.ack(usage.id, "raw", raw);
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    reply
}

fn usage_text(rig: &Rig) -> String {
    let reply = rig.pool(&["usage"], None);
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    reply.text
}

#[test]
fn samples_are_taken_every_15_minutes_and_land_locally_and_at_the_hub() {
    let rig = sampling(&[]);
    assert!(usage_text(&rig).starts_with("no plan usage sample yet"));
    let reply = answer_usage(&rig, OK_PLAN);
    assert!(reply.text.contains("Weekly · all models 42%"), "{}", reply.text);

    let imports = rig.hub.metrics_imports();
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0].source, SOURCE);
    assert_eq!(imports[0].rows.len(), 2);
    assert_eq!(imports[0].rows[0]["window"], "Weekly · all models");
    assert_eq!(imports[0].rows[0]["percent_used"], 42.5);

    rig.clock.advance(60.0);
    let text = usage_text(&rig);
    assert!(text.contains("5-hour 7%"), "{text}");
    assert!(text.contains("Weekly · all models 42%"), "{text}");
    assert!(text.contains("60 s ago, resets 2026-11-24T08:00:00Z"), "{text}");

    rig.clock.advance(USAGE_EVERY_S - 120.0);
    assert_eq!(rig.step(), None, "no sample before 15 minutes");
    rig.clock.advance(60.0);
    answer_usage(&rig, OK_PLAN);
    assert_eq!(rig.hub.metrics_imports().len(), 2, "only the new rows are sent");
    assert_eq!(rig.hub.metrics_imports()[1].rows.len(), 2);
}

#[test]
fn a_handoff_is_preceded_by_a_sample_older_than_5_minutes() {
    let rig = sampling(&["local_w1", "local_w2"]);
    answer_usage(&rig, OK_PLAN);

    rig.clock.advance(USAGE_BEFORE_HANDOFF_S - 60.0);
    rig.reserve("TA", "local_w1", &[]);
    let handoff = rig.step().unwrap();
    assert_eq!(handoff_to(&handoff), ("local_w1", "TA"), "the sample is fresh");

    rig.clock.advance(60.0);
    rig.reserve("TB", "local_w2", &[]);
    answer_usage(&rig, OK_PLAN);
    let handoff = rig.step().unwrap();
    assert_eq!(handoff_to(&handoff), ("local_w2", "TB"));
}

#[test]
fn a_status_other_than_ok_is_a_sample_with_no_windows() {
    let rig = sampling(&[]);
    answer_usage(&rig, OK_PLAN);
    rig.clock.advance(USAGE_EVERY_S);
    rig.hub.set_unreachable(true);
    answer_usage(
        &rig,
        r#"{"plan": {"status": "unavailable", "note": "usage could not be read"}}"#,
    );
    let text = usage_text(&rig);
    assert!(text.contains("unavailable (usage could not be read)"), "{text}");
    assert!(text.contains("Weekly · all models 42%"), "{text}");

    // The hub missed the sample, so it goes with the next one.
    rig.hub.set_unreachable(false);
    rig.clock.advance(USAGE_EVERY_S);
    answer_usage(&rig, "Error: no such tool");
    let imports = rig.hub.metrics_imports();
    let rows = &imports.last().unwrap().rows;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["status"], "unavailable");
    assert_eq!(rows[0]["window"], serde_json::Value::Null);
    assert_eq!(rows[1]["status"], "unreadable");
    assert_eq!(rows[1]["note"], "Error: no such tool");
}

#[test]
fn a_codex_dispatcher_is_never_asked() {
    let rig = rig_sampling(
        &[],
        ignite_store::local::open_in_memory().unwrap(),
        Timestamp::from_millis(START),
        true,
    );
    let reply = rig.pool(
        &["add", "local_codex", "--app", "codex", "--dispatcher", "--slots", "1"],
        Some("local_codex"),
    );
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert_eq!(rig.step(), None);
    rig.clock.advance(USAGE_EVERY_S * 2.0);
    assert_eq!(rig.step(), None);
}
