//! Drive the live server as two independent browser seats.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};

use beryl_session::GameUpdate;
use beryl_session::ipc::{self, TableLink};
use beryl_session::protocol::TableAction;
use beryl_table::action::Adjustment;
use beryl_table::{Action, Seat};
use serde_json::{Value, json};

struct Server {
    child: Child,
    address: String,
    _data: tempfile::TempDir,
}

impl Server {
    fn start() -> Self {
        let data = tempfile::tempdir().unwrap();
        // The test and CLI may be restored from another worktree's build cache.
        let test_binary = std::env::current_exe().unwrap();
        let cli = test_binary
            .parent()
            .unwrap()
            .with_file_name(format!("beryl_cli{}", std::env::consts::EXE_SUFFIX));
        let child = Command::new(cli)
            .args(["serve", "--port", "0"])
            .env("TMPDIR", data.path())
            .env("TMP", data.path())
            .env("TEMP", data.path())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut server = Self {
            child,
            address: String::new(),
            _data: data,
        };
        let mut line = String::new();
        BufReader::new(server.child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        server.address = line
            .trim_end()
            .strip_prefix("beryl_cli: serving on http://")
            .expect("the server must report its bound address")
            .into();
        server
    }

    fn invoke(&self, seat: &str, command: &str, args: &Value) -> Value {
        let mut stream = TcpStream::connect(&self.address).unwrap();
        let args = args.to_string();
        write!(
            stream,
            "POST /invoke/{command}?session={seat} HTTP/1.1\r\nContent-Length: {}\r\n\r\n{args}",
            args.len()
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
        serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
    }

    fn events(&self, seat: &str) -> BufReader<TcpStream> {
        let mut stream = TcpStream::connect(&self.address).unwrap();
        write!(stream, "GET /events?session={seat} HTTP/1.1\r\n\r\n").unwrap();
        let mut reader = BufReader::new(stream);
        assert_eq!(line(&mut reader), "HTTP/1.1 200 OK");
        while !line(&mut reader).is_empty() {}
        reader
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn line(reader: &mut impl BufRead) -> String {
    let mut line = String::new();
    assert_ne!(reader.read_line(&mut line).unwrap(), 0);
    line.trim_end().into()
}

fn event(reader: &mut impl BufRead, name: &str) -> Value {
    let expected = format!("event: {name}");
    loop {
        let header = line(reader);
        let payload = serde_json::from_str(line(reader).strip_prefix("data: ").unwrap()).unwrap();
        assert!(line(reader).is_empty());
        if header == expected {
            return payload;
        }
        assert_eq!(header, format!("event: {}", ipc::TABLE_LOBBY));
    }
}

fn update(reader: &mut impl BufRead) -> GameUpdate {
    serde_json::from_value(event(reader, ipc::GAME_UPDATE)).unwrap()
}

fn reject_bad_invites(server: &Server, invite: &str) {
    let (prefix, secret) = invite.rsplit_once(':').unwrap();
    let changed = if secret.starts_with('a') { 'b' } else { 'a' };
    let wrong = format!("{prefix}:{changed}{}", &secret[1..]);
    for invalid in ["not an invite", &wrong] {
        let reply = server.invoke(
            "guest",
            ipc::TABLE_JOIN,
            &json!({"request": {"name": "Guest", "invite": invalid}}),
        );
        assert!(reply["err"]["reason"].is_string(), "{reply}");
        assert_eq!(
            server.invoke("guest", ipc::TABLE_CURRENT, &json!({})),
            json!({"ok": null})
        );
    }
}

fn play_between_browser_seats(server: &Server, link: TableLink) {
    let mut host_events = server.events("host");
    let mut guest_events = server.events("guest");
    let host = server.invoke(
        "host",
        ipc::TABLE_HOST,
        &json!({
            "request": { "link": link, "settings": { "name": "Host", "startingLife": 25 } }
        }),
    );
    let invite = host["ok"]["invite"].as_str().unwrap();
    let initial = update(&mut host_events);
    assert_eq!(initial.view.players.you.life, 25);
    assert_eq!(event(&mut host_events, ipc::TABLE_LINK)["kind"], "alone");
    let lobby = event(&mut host_events, ipc::TABLE_LOBBY);
    assert_eq!(lobby["view"]["you"], 0);
    assert_eq!(lobby["view"]["seats"][0]["name"], "Host");
    assert_eq!(
        server.invoke("guest", ipc::TABLE_CURRENT, &json!({})),
        json!({"ok": null})
    );

    reject_bad_invites(server, invite);

    let joined = server.invoke(
        "guest",
        ipc::TABLE_JOIN,
        &json!({"request": {"name": "Guest", "invite": invite}}),
    );
    assert!(joined.get("ok").is_some(), "{joined}");
    let guest = update(&mut guest_events);
    assert_eq!(guest.view.table.unwrap().seat, Some(1));
    assert_eq!(
        event(&mut guest_events, ipc::TABLE_LINK)["kind"],
        "connected"
    );
    assert_eq!(
        event(&mut host_events, ipc::TABLE_LINK)["kind"],
        "connected"
    );
    assert_eq!(update(&mut host_events).session, initial.session);

    let action: TableAction = Action::Life {
        seat: Seat(1),
        adjustment: Adjustment::Add(3),
    };
    let reply = server.invoke(
        "guest",
        ipc::TABLE_ACT,
        &json!({"request": {"action": action}}),
    );
    assert_eq!(reply, json!({"ok": {"ok": true}}));
    assert_eq!(update(&mut host_events).view.players.opponent.life, 28);
    assert_eq!(update(&mut guest_events).view.players.you.life, 28);

    let listing = server.invoke("host", ipc::TABLE_SAVED, &json!({}));
    let saved = listing["ok"]
        .as_array()
        .unwrap()
        .iter()
        .find(|save| save["guest"] == "Guest")
        .unwrap();
    let id = saved["id"].clone();
    assert_eq!(saved["host"], "Host");
    assert!(saved.get("invite").is_none());
    assert_eq!(
        server.invoke("host", ipc::TABLE_LEAVE, &json!({})),
        json!({"ok": null})
    );
    assert_eq!(event(&mut host_events, ipc::TABLE_LINK)["kind"], "left");
    assert_eq!(event(&mut guest_events, ipc::TABLE_LINK)["kind"], "left");
    assert_eq!(
        server.invoke("guest", ipc::TABLE_LEAVE, &json!({})),
        json!({"ok": null})
    );

    let resumed = server.invoke("host", ipc::TABLE_RESUME, &json!({"request": {"id": id}}));
    assert_eq!(resumed, json!({"ok": {"invite": invite}}));
    let replayed = update(&mut host_events);
    assert_eq!(replayed.view.players.opponent.life, 28);
    let snapshot = replayed.view.script.unwrap();
    assert!(snapshot.reset.is_some());
    assert_eq!(snapshot.steps(), 0);
    assert_eq!(
        event(&mut host_events, ipc::TABLE_LINK)["kind"],
        "reconnecting"
    );
    assert!(
        server
            .invoke(
                "guest",
                ipc::TABLE_JOIN,
                &json!({"request": {"name": "Guest", "invite": invite}}),
            )
            .get("ok")
            .is_some()
    );
    assert_eq!(update(&mut guest_events).view.players.you.life, 28);
}

#[test]
fn linked_tables_exchange_actions_over_tcp_and_memory() {
    for link in [TableLink::Local, TableLink::Memory] {
        play_between_browser_seats(&Server::start(), link);
    }
}

#[test]
fn browser_relay_settings_are_scoped_to_the_selected_profile() {
    let server = Server::start();
    let empty = json!({"ok": {"relay": null}});
    let settings = json!({"relay": "https://relay.example.test"});
    assert_eq!(
        server.invoke("host", ipc::TABLE_SETTINGS_GET, &json!({})),
        empty
    );
    assert_eq!(
        server.invoke(
            "host",
            ipc::TABLE_SETTINGS_SET,
            &json!({"request": settings})
        ),
        json!({"ok": null})
    );
    assert!(
        server.invoke(
            "host",
            ipc::TABLE_SETTINGS_SET,
            &json!({"request": {"relay": "invalid"}})
        )["err"]["reason"]
            .is_string()
    );
    assert_eq!(
        server.invoke("host", ipc::TABLE_SETTINGS_GET, &json!({})),
        json!({"ok": settings})
    );
    assert_eq!(
        server.invoke("guest", ipc::TABLE_SETTINGS_GET, &json!({})),
        empty
    );
    assert_eq!(
        server.invoke(
            "host",
            ipc::TABLE_HOST,
            &json!({"request": {"link": "iroh", "settings": {"name": "Host"}}})
        ),
        json!({"err": {"reason": "This build has no internet play."}})
    );
}
