//! Editing tickets from the dashboard: the private listener takes ticket edits from a Tailscale
//! user's browser and shows the edit controls, and the public listener does neither.
#![allow(clippy::format_collect)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};

use serde_json::{Value, json};

use ignite_core::config::{Config, Os};
use ignite_core::fleet::{
    Allocations, FleetSettings, FleetUpdate, FleetView, MachineState, ProviderSample,
    ProviderSetting,
};
use ignite_core::protocol::hub::MachineRegister;
use ignite_core::shares::Source;
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

    let page = format!("/{id}");
    let (status, parked) = send(private.address(), "GET", &page, &BROWSER[..1], "");
    assert_eq!(status, 200);
    assert!(parked.contains("0 · Parked"));
    assert!(parked.contains("data-edit=\"allocation\""));
    assert!(!parked.contains("data-edit=\"priority\""));
    assert!(!parked.contains("data-action=\"release\""));

    // An allocation makes the ticket eligible and is recorded with the other edits.
    let edits = json!([
        {"op": "update", "id": id, "set": {"allocation": 300}},
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
    assert_eq!(detail["allocation"], 300);
    assert_eq!(
        detail["labels"],
        json!(["provider:codex", "tier:sonnet", "machine:m1"])
    );
    // The edit is recorded under the login's name, never the whole address, since the public
    // listener shows the changelog.
    assert!(!detail.to_string().contains("example.com"), "{detail}");
    assert!(detail.to_string().contains("\"someone\""), "{detail}");

    // The private listener shows the controls to a Tailscale user; the public one never does.
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
    // The public listener serves only the change feed of the API, not the fleet.
    assert_eq!(send(public.address(), "GET", FLEET_PATH, &[], "").0, 404);
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
fn the_first_private_start_seeds_fleet_defaults_only_once() {
    let definitions = r#"
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
[pool.cli.codex]
program = "codex"
[pool.cli.claude]
program = "claude"
[machine.jarvis.resources]
oracle = 1
[machine.cygnus]
os = "windows"
families = ["rust"]
"#;
    let dir = ignite_core::test_files::temp_dir()
        .join(format!("ignite-fleet-seed-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut server = HubServer {
        config: Config::parse("ignite.toml", definitions).unwrap(),
        listen: "127.0.0.1:0".to_owned(),
        database: dir.join("hub.sqlite"),
        wordmark: None,
    };
    // The public listener serves no fleet route, so the database shows it seeded nothing.
    let public = server.start_read_only().unwrap();
    let conn = hub::open(&server.database).unwrap();
    assert_eq!(hub::fleet_settings::settings(&conn).unwrap().version, 0);
    drop(conn);
    public.stop();

    let private = server.start().unwrap();
    let seeded = get_fleet(private.address());
    assert_eq!(seeded.settings.version, 1);
    assert_eq!(seeded.changes.len(), 1);
    assert_eq!(seeded.changes[0].settings, seeded.settings);
    assert_eq!(seeded.settings.machines.len(), 2);
    for name in ["jarvis", "cygnus"] {
        let setting = &seeded.settings.machines[name];
        assert_eq!(setting.state, MachineState::Active);
        assert_eq!(setting.trees, ignite_core::fleet::TreeSettings::default());
    }
    assert_eq!(seeded.settings.providers.len(), 2);
    for name in ["claude", "codex"] {
        assert_eq!(seeded.settings.providers[name], ProviderSetting::default());
    }
    private.stop();

    server.config.pool.cli.clear();
    let restarted = server.start().unwrap();
    assert_eq!(get_fleet(restarted.address()), seeded);
    restarted.stop();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn allocation_controls_round_trip_fleet_and_epic_weights() {
    let dir = ignite_core::test_files::temp_dir()
        .join(format!("ignite-allocation-edit-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let server = HubServer {
        config: Config::parse("ignite.toml", CONFIG).unwrap(),
        listen: "127.0.0.1:0".to_owned(),
        database: dir.join("hub.sqlite"),
        wordmark: None,
    };
    let private = server.start().unwrap();
    let address = private.address();
    let (_, pool) = send(address, "GET", "/pool", &BROWSER[..1], "");
    assert!(pool.contains("data-edit=\"general\""));
    assert!(pool.contains("data-edit=\"flaky\""));
    assert!(pool.contains("Inherited (1)"));

    check_empty_ticket_category_allocations(address);

    let mut desired = get_fleet(address).settings;
    desired.allocations = Some(Allocations {
        general: Some(100),
        sources: [(Source::Flaky, 0)].into(),
    });
    assert_eq!(put_fleet(address, BROWSER, &desired).0, 200);
    let (status, feed) = send(address, "GET", "/pj/api/feed?since=0", &[], "");
    assert_eq!(status, 200);
    assert!(feed.contains("\"kind\":\"allocation\""));
    let mut current = get_fleet(address).settings;
    assert_eq!(current.allocations, desired.allocations);
    current.allocations = None; // A client from before allocation updates an unrelated setting.
    current
        .machines
        .get_mut("m1")
        .unwrap()
        .trees
        .tree_ceiling_gb = 200;
    assert_eq!(put_fleet(address, BROWSER, &current).0, 200);
    assert_eq!(get_fleet(address).settings.allocations, desired.allocations);
    let (_, pool) = send(address, "GET", "/pool", &BROWSER[..1], "");
    assert!(pool.contains("0 · Parked"));
    assert!(pool.contains("Inherited (100)"));
    let (_, read_only) = send(address, "GET", "/pool", &[], "");
    assert!(!read_only.contains("data-edit=\"general\""));
    check_ticket_category_allocations(address);

    let batch = |ops: Value| {
        let (status, body) = send(
            address,
            "POST",
            "/pj/api/batch",
            BROWSER,
            &json!({"ops":ops}).to_string(),
        );
        assert_eq!(status, 200, "{body}");
        serde_json::from_str::<Value>(&body).unwrap()
    };
    let reply = batch(json!([
        {"op":"create","fields":{"type":"epic","title":"Planned epic"}},
        {"op":"create","parent":"$0","fields":{"type":"milestone","title":"Nested milestone"}},
        {"op":"create","parent":"$1","fields":{"title":"Queued task"}},
        {"op":"create","parent":"$1","fields":{"title":"Parked task","allocation":0}}
    ]));
    let id = |n: usize| reply["results"][n]["id"].as_str().unwrap();
    let detail = |n| {
        let (status, body) = send(
            address,
            "GET",
            &format!("/pj/api/tickets/{}", id(n)),
            &[],
            "",
        );
        assert_eq!(status, 200, "{body}");
        serde_json::from_str::<Value>(&body).unwrap()
    };
    assert_eq!(detail(2)["status"], "pending_approval");
    batch(json!([{"op":"update","id":id(0),"set":{"allocation":200}}]));
    assert_eq!(detail(0)["allocation"], 200);
    assert_eq!(detail(0)["workers"], 200);
    assert_eq!(detail(2)["status"], "open");
    assert_eq!(detail(3)["allocation"], 0);
    let (_, milestone) = send(address, "GET", &format!("/{}", id(1)), &BROWSER[..1], "");
    assert!(milestone.contains("Inherited (200)"));

    batch(json!([{"op":"update","id":id(0),"set":{"workers":100}}]));
    assert_eq!(detail(0)["allocation"], 100);
    batch(json!([{"op":"update","id":id(2),"set":{"priority":1}}]));
    assert_eq!(detail(2)["allocation"], 200);
    batch(json!([{"op":"update","id":id(2),"set":{"allocation":null}}]));
    assert_eq!(detail(2)["allocation"], Value::Null);
    assert_eq!(detail(2)["priority"], 2);

    private.stop();
    let _ = std::fs::remove_dir_all(dir);
}

fn check_empty_ticket_category_allocations(address: SocketAddr) {
    // All categories can be edited before there are any tickets in them.
    let (_, tickets) = send(address, "GET", "/", &BROWSER[..1], "");
    let standalone_header = tickets
        .split_once("id=\"tickets\"")
        .unwrap()
        .1
        .split_once("</h3>")
        .unwrap()
        .0;
    assert!(standalone_header.contains("Standalone tickets"));
    assert!(standalone_header.contains("data-edit=\"general\""));
    assert!(standalone_header.contains(">1</span>"));
    let automatic = tickets.split_once("id=\"automatic\"").unwrap().1;
    for source in Source::ALL {
        assert!(automatic.contains(&format!("data-edit=\"{}\"", source.key())));
    }
}

fn check_ticket_category_allocations(address: SocketAddr) {
    let (_, tickets) = send(address, "GET", "/?fragment=all", &BROWSER[..1], "");
    let flaky_header = tickets
        .split_once(">Flaky checks")
        .unwrap()
        .1
        .split_once("</h4>")
        .unwrap()
        .0;
    assert!(flaky_header.contains("data-edit=\"flaky\""));
    assert!(flaky_header.contains("Allocation · Parked"));
    assert!(flaky_header.contains(">0</span>"));
    assert!(tickets.contains(">100</span>"));
    let (_, read_only) = send(address, "GET", "/", &[], "");
    assert!(read_only.contains("Allocation · Parked"));
    assert!(!read_only.contains("data-edit=\"general\""));
    assert!(!read_only.contains("id=\"c-allocation-fleet\""));
}
