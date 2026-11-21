//! Exercise plan usage import, heartbeats and dispatch through the hub routes.

use rusqlite::Connection;
use serde_json::{Value, json};

use ignite_core::fleet::{FleetSettings, ProviderSetting};
use ignite_store::hub::{fleet_settings, open_in_memory};

use super::Request;
use super::tests::{at, file, free, post, register};
use crate::auth::Writer;

fn sample(
    conn: &mut Connection,
    machine: &str,
    provider: &str,
    window: &str,
    percent: f64,
    seconds: f64,
) {
    let body = json!({
        "machine": machine, "source": "plan_usage", "rows": [{
            "provider": provider, "window": window, "percent_used": percent,
            "status": "ok", "at": at(seconds), "resets_at": at(60.0)
        }]
    });
    let request = Request {
        method: "POST".to_owned(),
        path: "/metrics/import".to_owned(),
        query: Vec::new(),
        writer: Some(Writer {
            machine: machine.parse().unwrap(),
            actor: "test".to_owned(),
        }),
        body: serde_json::to_vec(&body).unwrap(),
        headers: Vec::new(),
    };
    let reply = crate::routes::checks::handle(conn, &request, at(seconds)).unwrap();
    assert_eq!(reply.status, 200);
}

fn setup() -> Connection {
    let mut conn = open_in_memory().unwrap();
    register(&mut conn);
    let registration = json!({"machine": "m-2", "name": "multi", "os": "macos", "cores": 18,
                              "memory_mb": 65536, "families": [], "version": "d97ed0e"});
    assert_eq!(
        post(&mut conn, "/machines/register", &registration, at(0.0)).0,
        200
    );
    let mut settings = FleetSettings::default();
    settings.providers.insert(
        "codex".to_owned(),
        ProviderSetting {
            stop_percent: 90,
            ..ProviderSetting::default()
        },
    );
    fleet_settings::put(&mut conn, 0, settings, "operator", at(0.0)).unwrap();
    conn
}

fn codex_free() -> Value {
    let mut request = free("m-2", &["cli-codex-slot-1"]);
    request["idle_workers"][0]["app"] = json!("codex");
    request["explain"] = json!(true);
    request
}

#[test]
fn a_sample_holds_only_its_provider_on_another_machine_until_the_reset() {
    let mut conn = setup();
    let ids = file(&mut conn, &[("one", &[], &[]), ("two", &[], &[])]);
    sample(&mut conn, "m-1", "codex", "5-hour", 95.0, 1.0);
    let request = codex_free();
    let (_, reply) = post(&mut conn, "/next", &request, at(2.0));
    assert!(
        reply["nothing"]
            .as_str()
            .unwrap()
            .contains("codex is on a usage hold"),
        "{reply}"
    );
    let (_, heartbeat) = post(
        &mut conn,
        "/machines/heartbeat",
        &json!({
            "machine": "m-2", "version": "d97ed0e", "workers": []
        }),
        at(2.0),
    );
    assert_eq!(
        heartbeat["fleet"]["holds"]["codex"][0]["percent_used"],
        95.0
    );
    assert!(heartbeat["fleet"]["holds"].get("claude").is_none());
    let (_, reply) = post(&mut conn, "/next", &free("m-2", &["claude-idle"]), at(3.0));
    assert_eq!(reply["ticket"], ids[0].as_str());
    let (_, reply) = post(&mut conn, "/next", &request, at(60.0));
    assert_eq!(reply["ticket"], ids[1].as_str());
}

#[test]
fn an_older_upload_cannot_undo_a_newer_lower_sample() {
    let mut conn = setup();
    let id = file(&mut conn, &[("one", &[], &[])])[0].clone();
    sample(&mut conn, "m-1", "codex", "5-hour", 95.0, 1.0);
    sample(&mut conn, "m-2", "codex", "5-hour", 89.0, 3.0);
    sample(&mut conn, "m-1", "codex", "5-hour", 97.0, 2.0);
    let (_, reply) = post(&mut conn, "/next", &codex_free(), at(4.0));
    assert_eq!(reply["ticket"], id.as_str());
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM metrics WHERE source = 'plan_usage'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 3, "every sample is still available to charts");
}

#[test]
fn a_family_hold_uses_the_worker_model_ids_including_the_default() {
    let mut conn = setup();
    let ids = file(
        &mut conn,
        &[
            ("untiered", &[], &[]),
            ("light", &["tier:light"], &[]),
            ("opus", &["tier:opus"], &[]),
        ],
    );
    sample(&mut conn, "m-1", "claude", "Weekly · Sonnet", 99.0, 1.0);
    let mut request = free("m-2", &["claude-idle"]);
    request["idle_workers"][0]["models"] = json!(["light", "opus"]);
    request["idle_workers"][0]["model_ids"] =
        json!({"light": "claude-sonnet", "opus": "claude-opus"});
    request["idle_workers"][0]["default_model"] = json!("claude-sonnet");
    let (_, reply) = post(&mut conn, "/next", &request, at(2.0));
    assert_eq!(reply["ticket"], ids[2].as_str());
}
