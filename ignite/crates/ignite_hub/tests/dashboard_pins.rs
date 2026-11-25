//! Effective pins on ticket pages and a ticket's own pins on its card.
#![allow(clippy::format_collect)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};

use serde_json::{Value, json};

use ignite_core::config::{Config, Os};
use ignite_core::protocol::hub::MachineRegister;
use ignite_core::time::Timestamp;
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

[pool.models.claude]
opus = "o"

[pool.models.codex]
opus = "sol"

[pool.cli.claude]
program = "claude"

[pool.cli.codex]
program = "codex"

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
fn ticket_properties_show_effective_pins_and_cards_show_own_pins() {
    let dir = ignite_core::test_files::temp_dir()
        .join(format!("ignite-dashboard-pins-{}", std::process::id()));
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

    let conn = ignite_store::hub::open(&server.database).unwrap();
    for (name, os) in [("jarvis", Os::Macos), ("cygnus", Os::Windows)] {
        ignite_store::hub::fleet::register(
            &conn,
            &MachineRegister {
                machine: format!("m-{name}").parse().unwrap(),
                name: name.to_owned(),
                os,
                cores: 8,
                memory_mb: 8_192,
                families: Vec::new(),
                version: "test".to_owned(),
            },
            Timestamp::EPOCH,
        )
        .unwrap();
    }
    drop(conn);

    // An epic pins both values. Its child overrides only the provider.
    let create = json!({"ops": [
        {"op": "create", "fields": {"title": "Windows work", "type": "epic",
                                    "labels": ["machine:cygnus", "provider:codex"]}},
        {"op": "create", "parent": "$0", "fields": {"title": "Port it", "type": "task", "labels": ["provider:claude"]}},
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

    let (_, child) = send(public.address(), "GET", &format!("/{}", id(1)), &[], "");
    assert!(child.contains("<dt>Machine</dt>") && child.contains("<dt>Provider</dt>"));
    assert!(child.contains("cygnus <span class=\"c-muted\">inherited</span>"));
    assert!(child.contains("claude"));
    let (_, loose) = send(public.address(), "GET", &format!("/{}", id(2)), &[], "");
    assert_eq!(
        loose
            .matches("<dd><span class=\"c-muted\">None</span></dd>")
            .count(),
        2
    );
    let (_, home) = send(public.address(), "GET", "/", &[], "");
    // The epic's own pins: its machine, and the model its pinned provider runs (`[pool]` is
    // not given here, so the tier's name stands for the model).
    assert!(home.contains(">Cygnus</span>") && home.contains(">sonnet</span>"));

    let (_, editable) = send(
        private.address(),
        "GET",
        &format!("/{}", id(1)),
        &BROWSER[..1],
        "",
    );
    for field in ["machine", "provider"] {
        assert!(editable.contains(&format!("data-field=\"{field}\" data-kind=\"choice\"")));
        assert!(editable.contains(&format!("data-edit=\"{field}\"")));
    }
    assert!(editable.contains("[&quot;&quot;,&quot;None&quot;]"));
    assert!(editable.contains("[&quot;machine:jarvis&quot;,&quot;jarvis&quot;]"));
    assert!(editable.contains("[&quot;provider:codex&quot;,&quot;codex&quot;]"));
    assert!(editable.contains("class=\"c-edit-source\">provider:claude</template>"));
    assert!(
        editable.contains("class=\"c-edit-pin-labels\">[&quot;provider:claude&quot;]</template>")
    );
    assert!(editable.contains("data-field=\"labels\""));
    assert!(!editable.contains("data-field=\"owner\""));

    public.stop();
    private.stop();
    let _ = std::fs::remove_dir_all(dir);
}
