//! The read-only listener that is published on the public internet: it serves reads and
//! refuses every write, including a machine's first registration, which needs no token on the
//! private listener.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};

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

/// The status code of one request.
fn status(address: SocketAddr, method: &str, path: &str, body: &str) -> u16 {
    let message = format!(
        "{method} {path} HTTP/1.0\r\nHost: hub\r\nConnection: close\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let mut stream = TcpStream::connect(address).unwrap();
    stream.write_all(message.as_bytes()).unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    reply.split(' ').nth(1).unwrap().parse().unwrap()
}

#[test]
fn the_public_listener_serves_reads_and_refuses_writes_and_export() {
    let dir = ignite_core::test_files::temp_dir()
        .join(format!("ignite-read-only-{}", std::process::id()));
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
    let register = r#"{"machine":"intruder"}"#;

    assert_eq!(status(public.address(), "GET", "/pj/api/tickets", ""), 200);
    assert_eq!(status(public.address(), "HEAD", "/", ""), 200);
    assert_eq!(status(public.address(), "GET", "/", ""), 200);
    // The project's old page address redirects to the root.
    assert_eq!(status(public.address(), "GET", "/pj/", ""), 308);
    for (method, path) in [
        ("POST", "/pj/api/machines/register"),
        ("POST", "/pj/api/batch"),
        ("PUT", "/pj/api/tickets/x"),
        ("DELETE", "/pj/api/tickets/x"),
    ] {
        assert_eq!(status(public.address(), method, path, register), 405);
    }
    assert_eq!(status(public.address(), "GET", "/pj/api/export", ""), 404);
    assert_eq!(status(public.address(), "GET", "/pj/api/export/x", ""), 404);
    // The private listener still serves the export and takes writes.
    assert_eq!(status(private.address(), "GET", "/pj/api/export", ""), 200);
    assert_ne!(
        status(
            private.address(),
            "POST",
            "/pj/api/machines/register",
            register
        ),
        405
    );

    public.stop();
    private.stop();
    let _ = std::fs::remove_dir_all(dir);
}
