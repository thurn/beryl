//! Editing tickets from the dashboard: the private listener takes ticket edits from a Tailscale
//! user's browser and shows the edit controls, and the public listener does neither.
#![allow(clippy::format_collect)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};

use serde_json::{Value, json};

use ignite_core::config::{Config, Os};
use ignite_core::fleet::{
    FleetSettings, FleetUpdate, FleetView, MachineState, ProviderSample, ProviderSetting,
};
use ignite_core::protocol::hub::MachineRegister;
use ignite_core::time::Timestamp;
use ignite_hub::server::HubServer;
use ignite_store::hub;

const CONFIG: &str = r#"
[repository]
remote = "origin"
staging = "staging"
master = "master"
trees = "trees"
checks = "checks"

[hub]
url = "http://127.0.0.1:0"
project = "pj"

[gate]
threshold = 0.004
cap = 400

[machine.m1]
os = "macos"
ticket_trees = 1
landing_trees = 1
trees_disk_gb = 10
floor = { batch = 1, tip = 1 }
"#;

/// What a browser on the private network sends: the login `tailscale serve` adds and the header
/// the dashboard's script adds.
const BROWSER: &[(&str, &str)] = &[
    ("Tailscale-User-Login", "someone@example.com"),
    ("X-Ignite-Dashboard", "1"),
];

/// One request's status code and body.
fn send(
    address: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> (u16, String) {
    let extra: String = headers
        .iter()
        .map(|(name, value)| format!("{name}: {value}\r\n"))
        .collect();
    let message = format!(
        "{method} {path} HTTP/1.0\r\nHost: hub\r\nConnection: close\r\n{extra}\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let mut stream = TcpStream::connect(address).unwrap();
    stream.write_all(message.as_bytes()).unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    let status = reply.split(' ').nth(1).unwrap().parse().unwrap();
    let body = reply
        .split_once("\r\n\r\n")
        .map_or("", |(_, body)| body)
        .to_owned();
    (status, body)
}

#[test]
fn a_tailscale_user_edits_tickets_on_the_private_listener_only() {
    let dir = ignite_core::test_files::temp_dir()
        .join(format!("ignite-dashboard-edit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let server = HubServer {
        config: Config::parse("ignite.toml", CONFIG).unwrap(),
        listen: "127.0.0.1:0".to_owned(),
        database: dir.join("hub.sqlite"),
        wordmark: None,
    };
    let public = server.start_read_only().unwrap();
    let private = server.start().unwrap();
    let batch = |listener: SocketAddr, headers: &[(&str, &str)], ops: Value| {
        send(
            listener,
            "POST",
            "/pj/api/batch",
            headers,
            &json!({ "ops": ops }).to_string(),
        )
    };

    let create = json!([{"op": "create", "fields": {
        "title": "Edit me", "type": "task", "status": "pending_approval",
        "labels": ["machine:old", "machine:older", "provider:codex", "tier:sonnet"]}}]);
    let (status, reply) = batch(private.address(), BROWSER, create.clone());
    assert_eq!(status, 200, "{reply}");
    let reply: Value = serde_json::from_str(&reply).unwrap();
    let id = reply["results"][0]["id"].as_str().unwrap().to_owned();

    // Without the script's header (a form posted from another site) or without a Tailscale
    // login, the write is refused; the public listener refuses every write.
    assert_eq!(
        batch(private.address(), &BROWSER[..1], create.clone()).0,
        401
    );
    assert_eq!(
        batch(private.address(), &BROWSER[1..], create.clone()).0,
        401
    );
    assert_eq!(batch(public.address(), BROWSER, create).0, 405);
    // The login authorizes ticket edits only, not the rest of the API.
    let (status, _) = send(
        private.address(),
        "POST",
        &format!("/pj/api/leases/{id}"),
        BROWSER,
        r#"{"op":"acquire","holder":"x","ttl_s":60}"#,
    );
    assert_eq!(status, 401);

    // Release it, raise its priority, replace its machine pin and comment.
    let edits = json!([
        {"op": "update", "id": id, "set": {"status": "open", "priority": 0}},
        {"op": "update", "id": id, "add_labels": ["machine:m1"],
         "remove_labels": ["machine:old", "machine:older"]},
        {"op": "comment", "id": id, "text": "Released from the dashboard"},
    ]);
    assert_eq!(batch(private.address(), BROWSER, edits).0, 200);
    let (_, detail) = send(
        private.address(),
        "GET",
        &format!("/pj/api/tickets/{id}?history=1"),
        &[],
        "",
    );
    let detail: Value = serde_json::from_str(&detail).unwrap();
    assert_eq!(detail["status"], "open", "{detail}");
    assert_eq!(detail["priority"], 0);
    assert_eq!(
        detail["labels"],
        json!(["provider:codex", "tier:sonnet", "machine:m1"])
    );
    // The edit is recorded under the login's name, never the whole address, since the public
    // listener shows the changelog.
    assert!(!detail.to_string().contains("example.com"), "{detail}");
    assert!(detail.to_string().contains("\"someone\""), "{detail}");

    // The private listener shows the controls to a Tailscale user; the public one never does.
    let page = format!("/{id}");
    let (status, private_page) = send(private.address(), "GET", &page, &BROWSER[..1], "");
    assert_eq!(status, 200);
    assert!(private_page.contains("id=\"c-edit\""), "{private_page}");
    assert!(private_page.contains("data-edit=\"body\""));
    assert!(private_page.contains("data-action=\"close\""));
    let (_, anonymous) = send(private.address(), "GET", &page, &[], "");
    assert!(!anonymous.contains("c-edit"));
    let (status, public_page) = send(public.address(), "GET", &page, &BROWSER[..1], "");
    assert_eq!(status, 200);
    assert!(!public_page.contains("c-edit"), "{public_page}");

    // The pages' script follows the change feed at the root on both listeners.
    for listener in [private.address(), public.address()] {
        let (status, _) = send(listener, "GET", "/api/feed?since=0&wait=0", &[], "");
        assert_eq!(status, 200);
    }

    public.stop();
    private.stop();
    let _ = std::fs::remove_dir_all(dir);
}

const FLEET_PATH: &str = "/pj/api/fleet";

fn get_fleet(address: SocketAddr) -> FleetView {
    let (status, body) = send(address, "GET", FLEET_PATH, &[], "");
    assert_eq!(status, 200, "{body}");
    serde_json::from_str(&body).unwrap()
}

fn put_fleet(
    address: SocketAddr,
    headers: &[(&str, &str)],
    settings: &FleetSettings,
) -> (u16, String) {
    let body = serde_json::to_string(&FleetUpdate {
        version: settings.version,
        settings: settings.clone(),
    })
    .unwrap();
    send(address, "PUT", FLEET_PATH, headers, &body)
}

#[test]
fn fleet_writes_use_the_private_listener_and_check_the_version() {
    let dir = ignite_core::test_files::temp_dir()
        .join(format!("ignite-fleet-edit-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let server = HubServer {
        config: Config::parse("ignite.toml", CONFIG).unwrap(),
        listen: "127.0.0.1:0".to_owned(),
        database: dir.join("hub.sqlite"),
        wordmark: None,
    };
    let private = server.start().unwrap();
    let public = server.start_read_only().unwrap();
    let machine = "m-fleet".parse().unwrap();
    let conn = hub::open(&server.database).unwrap();
    let token = hub::tokens::issue(&conn, &machine).unwrap();
    hub::fleet::register(
        &conn,
        &MachineRegister {
            machine: machine.clone(),
            name: "m1".to_owned(),
            os: Os::Macos,
            cores: 8,
            memory_mb: 8_192,
            families: Vec::new(),
            version: "first".to_owned(),
        },
        Timestamp::EPOCH,
    )
    .unwrap();
    let sample = ProviderSample {
        provider: "codex".to_owned(),
        at: Timestamp::EPOCH,
        machine,
        status: "ok".to_owned(),
        label: Some("Weekly".to_owned()),
        percent_used: Some(95.0),
        resets_at: Some("2099-01-01T00:00:00.000Z".to_owned()),
        note: None,
    };
    hub::provider_usage::record(&conn, &sample).unwrap();
    drop(conn);

    let before = get_fleet(private.address());
    assert_eq!(before, get_fleet(public.address()));
    assert_eq!(before.machines[0].name, "m1");
    assert_eq!(before.machines[0].id, sample.machine);
    assert_eq!(before.usage, [sample]);
    assert!(before.holds.is_empty());

    let bearer = format!("Bearer {token}");
    let machine_headers = [("Authorization", bearer.as_str()), ("X-Actor", "operator")];
    let mut desired = before.settings;
    desired
        .providers
        .entry("codex".to_owned())
        .or_default()
        .share = 50;
    for headers in [&BROWSER[..1], &BROWSER[1..], &[]] {
        assert_eq!(put_fleet(private.address(), headers, &desired).0, 401);
    }
    for headers in [BROWSER, machine_headers.as_slice()] {
        assert_eq!(put_fleet(public.address(), headers, &desired).0, 405);
    }

    let (status, body) = put_fleet(private.address(), &machine_headers, &desired);
    assert_eq!(status, 200, "{body}");
    let saved: FleetSettings = serde_json::from_str(&body).unwrap();
    assert_eq!(saved.version, desired.version + 1);
    assert_eq!(saved.provider("codex").share, 50);
    let (status, body) = put_fleet(private.address(), BROWSER, &desired);
    assert_eq!(status, 409, "{body}");
    assert_eq!(serde_json::from_str::<FleetSettings>(&body).unwrap(), saved);

    let mut desired = saved;
    desired.providers.get_mut("codex").unwrap().stop_percent = 101;
    assert_eq!(put_fleet(private.address(), BROWSER, &desired).0, 400);
    desired.providers.get_mut("codex").unwrap().stop_percent = 90;
    assert_eq!(put_fleet(private.address(), BROWSER, &desired).0, 200);
    let current = get_fleet(private.address());
    assert_eq!(current.holds["codex"][0].label, "Weekly");
    assert_eq!(current.changes.len(), 3);
    assert_eq!(current.changes[0].actor, "someone");
    assert_eq!(current.changes[1].actor, "operator");

    let mut settings = current.settings;
    for share in 51..=71 {
        settings.providers.get_mut("codex").unwrap().share = share;
        let (status, body) = put_fleet(private.address(), &machine_headers, &settings);
        assert_eq!(status, 200, "{body}");
        settings = serde_json::from_str(&body).unwrap();
    }
    let recent = get_fleet(private.address()).changes;
    assert_eq!(recent.len(), 20);
    assert_eq!(recent[0].settings, settings);
    assert_eq!(recent[19].settings.provider("codex").share, 52);

    public.stop();
    private.stop();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_first_private_start_seeds_the_legacy_fleet_only_once() {
    // The legacy fleet's pool and disk-cap values, independent of later edits to the repo config.
    let legacy = r#"
[repository]
remote = "origin"
staging = "staging"
master = "master"
trees = "trees"
checks = "checks"
[hub]
url = "http://127.0.0.1:0"
project = "pj"
[gate]
threshold = 0.004
cap = 400
[pool]
usage_hold_percent = 99
[pool.cli.codex]
program = "codex"
[pool.cli.claude]
program = "claude"
[machine.jarvis]
ticket_trees = 24
landing_trees = 3
trees_disk_gb = 900
[machine.jarvis.pool.codex]
mode = "cli"
slots = 15
[machine.multivac]
ticket_trees = 8
landing_trees = 2
trees_disk_gb = 350
[machine.multivac.pool.codex]
mode = "desktop"
slots = 3
[machine.cygnus]
ticket_trees = 3
landing_trees = 2
trees_disk_gb = 60
[machine.cygnus.pool.missing]
slots = 3
"#;
    let dir = ignite_core::test_files::temp_dir()
        .join(format!("ignite-fleet-seed-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut server = HubServer {
        config: Config::parse("ignite.toml", legacy).unwrap(),
        listen: "127.0.0.1:0".to_owned(),
        database: dir.join("hub.sqlite"),
        wordmark: None,
    };
    let public = server.start_read_only().unwrap();
    assert_eq!(get_fleet(public.address()).settings.version, 0);
    public.stop();

    let private = server.start().unwrap();
    let seeded = get_fleet(private.address());
    assert_eq!(seeded.settings.version, 1);
    assert_eq!(seeded.changes.len(), 1);
    assert_eq!(seeded.changes[0].settings, seeded.settings);
    assert_eq!(seeded.settings.machines.len(), 3);
    for (name, state, cap) in [
        ("jarvis", MachineState::Active, 900),
        ("multivac", MachineState::Active, 350),
        ("cygnus", MachineState::ChecksOnly, 60),
    ] {
        let setting = seeded.settings.machine(name);
        assert_eq!(setting.state, state);
        assert_eq!(setting.disk_cap_gb, Some(cap));
    }
    assert_eq!(seeded.settings.providers.len(), 2);
    assert_eq!(
        seeded.settings.provider("codex"),
        ProviderSetting {
            enabled: true,
            share: 18,
            stop_percent: 99,
        }
    );
    assert_eq!(
        seeded.settings.provider("claude"),
        ProviderSetting::default()
    );
    private.stop();

    server.config.pool.usage_hold_percent = 85;
    let restarted = server.start().unwrap();
    assert_eq!(get_fleet(restarted.address()), seeded);
    restarted.stop();
    let _ = std::fs::remove_dir_all(dir);
}
