//! Editing tickets from the dashboard: the private listener takes ticket edits from a Tailscale
//! user's browser and shows the edit controls, and the public listener does neither.
#![allow(clippy::format_collect)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};

use serde_json::{Value, json};

use ignite_core::config::Config;
use ignite_hub::server::HubServer;

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
    let dir = std::env::temp_dir().join(format!("ignite-dashboard-edit-{}", std::process::id()));
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
        "title": "Edit me", "type": "task", "status": "pending_approval"}}]);
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

    // Release it, raise its priority and comment, as the page's controls do.
    let edits = json!([
        {"op": "update", "id": id, "set": {"status": "open", "priority": 0}},
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
