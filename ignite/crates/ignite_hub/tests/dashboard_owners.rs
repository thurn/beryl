//! Ticket owners on the dashboard: the Owner row and dropdown of a ticket's page, the default
//! owner in the settings panel, and who may change it.
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

[pool]
default_owner = "jarvis/claude"

[pool.models.claude]
opus = "o"

[pool.models.codex]
opus = "sol"

[machine.jarvis]
os = "macos"
ticket_trees = 1
landing_trees = 1

[machine.cygnus]
os = "windows"
ticket_trees = 1
landing_trees = 1
"#;

const BROWSER: &[(&str, &str)] = &[
    ("Tailscale-User-Login", "someone@example.com"),
    ("X-Ignite-Dashboard", "1"),
];

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
fn owners_show_on_pages_and_a_ticket_without_one_goes_to_the_default_owner() {
    let dir = ignite_core::test_files::temp_dir().join(format!("ignite-dashboard-owners-{}", std::process::id()));
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

    // An epic owned by cygnus/codex, with one child that follows it.
    let create = json!({"ops": [
        {"op": "create", "fields": {"title": "Windows work", "type": "epic",
                                    "owner": "cygnus/codex"}},
        {"op": "create", "parent": "$0", "fields": {"title": "Port it", "type": "task"}},
        {"op": "create", "fields": {"title": "Anything", "type": "task"}},
    ]});
    let (status, reply) = send(
        private.address(),
        "POST",
        "/pj/api/batch",
        BROWSER,
        &create.to_string(),
    );
    assert_eq!(status, 200, "{reply}");
    let reply: Value = serde_json::from_str(&reply).unwrap();
    let id = |n: usize| reply["results"][n]["id"].as_str().unwrap().to_owned();

    // A child shows its epic's owner. A loose ticket with no owner shows the default owner.
    let (_, child) = send(public.address(), "GET", &format!("/{}", id(1)), &[], "");
    assert!(child.contains("cygnus · Codex"), "{child}");
    assert!(child.contains("from "));
    let (_, loose) = send(public.address(), "GET", &format!("/{}", id(2)), &[], "");
    assert!(loose.contains("jarvis · Claude"), "{loose}");
    assert!(loose.contains("by default"), "{loose}");
    // The epic's own card in a list carries its owner chip.
    let (_, home) = send(public.address(), "GET", "/", &[], "");
    assert!(home.contains("cygnus · Codex"), "{home}");

    // On the private listener the Owner row is a dropdown of None and the four pairs.
    let (_, editable) = send(
        private.address(),
        "GET",
        &format!("/{}", id(2)),
        &BROWSER[..1],
        "",
    );
    assert!(
        editable.contains("data-field=\"owner\" data-kind=\"choice\""),
        "{editable}"
    );
    assert!(editable.contains("=None"));
    assert!(editable.contains("cygnus/codex=cygnus · Codex"));
    // The settings panel offers no default owner, and its API is gone.
    assert!(!editable.contains("data-owners"));
    let (status, _) = send(public.address(), "GET", "/pj/api/owners", &[], "");
    assert_eq!(status, 404);

    public.stop();
    private.stop();
    let _ = std::fs::remove_dir_all(dir);
}
