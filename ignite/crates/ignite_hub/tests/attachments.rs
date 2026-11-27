//! Image attachments against real hub listeners: an upload through the API, what it refuses,
//! the file served back, and the ticket page, which shows the image on the private listener and
//! a line in its place on the public one, which serves no attachment file or route.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;

use serde_json::{Value, json};

use ignite_core::config::Config;
use ignite_core::protocol::attachments::MAX_ATTACHMENT_BYTES;
use ignite_hub::server::{HubServer, RunningHub};
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
attachment_budget_mb = 1

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

struct Reply {
    status: u16,
    head: String,
    body: Vec<u8>,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

fn send(address: SocketAddr, method: &str, path: &str, token: Option<&str>, body: &[u8]) -> Reply {
    let auth = token.map_or_else(String::new, |token| {
        format!("Authorization: Bearer {token}\r\nX-Actor: tester\r\n")
    });
    let head = format!(
        "{method} {path} HTTP/1.0\r\nHost: hub\r\nConnection: close\r\n{auth}\
         Content-Length: {}\r\n\r\n",
        body.len()
    );
    let mut stream = TcpStream::connect(address).unwrap();
    stream.write_all(head.as_bytes()).unwrap();
    // The hub may answer a refused body before reading it all.
    let _ = stream.write_all(body);
    let mut reply = Vec::new();
    stream.read_to_end(&mut reply).unwrap();
    let split = reply
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("a reply head");
    let head = String::from_utf8_lossy(&reply[..split]).into_owned();
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    Reply {
        status,
        head,
        body: reply[split + 4..].to_vec(),
    }
}

/// A PNG header of the given size, padded with `tag` so images are told apart.
fn png(width: u32, height: u32, tag: u8) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    bytes.extend(width.to_be_bytes());
    bytes.extend(height.to_be_bytes());
    bytes.extend([8, 6, 0, 0, 0, tag, tag, tag]);
    bytes
}

struct Hubs {
    dir: PathBuf,
    token: String,
    private: Option<RunningHub>,
    public: Option<RunningHub>,
}

impl Drop for Hubs {
    fn drop(&mut self) {
        self.private.take().unwrap().stop();
        self.public.take().unwrap().stop();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Hubs {
    fn start() -> Hubs {
        let dir = ignite_core::test_files::temp_dir().join("hub-attachments");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let conn = hub::open(&dir.join("hub.sqlite")).unwrap();
        let token = hub::tokens::issue(&conn, &"m-1".parse().unwrap()).unwrap();
        let server = HubServer {
            config: Config::parse("ignite.toml", CONFIG).unwrap(),
            listen: "127.0.0.1:0".to_owned(),
            database: dir.join("hub.sqlite"),
            wordmark: None,
        };
        Hubs {
            private: Some(server.start().unwrap()),
            public: Some(server.start_read_only().unwrap()),
            dir,
            token,
        }
    }

    fn private(&self) -> SocketAddr {
        self.private.as_ref().unwrap().address()
    }

    fn public(&self) -> SocketAddr {
        self.public.as_ref().unwrap().address()
    }

    fn create(&self, title: &str) -> String {
        let body = json!({"ops": [{"op": "create", "fields": {"title": title}}]});
        let reply = send(
            self.private(),
            "POST",
            "/pj/api/batch",
            Some(&self.token),
            body.to_string().as_bytes(),
        );
        assert_eq!(reply.status, 200, "{}", reply.text());
        reply.json()["results"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn upload(&self, id: &str, query: &str, bytes: &[u8]) -> Reply {
        send(
            self.private(),
            "POST",
            &format!("/pj/api/tickets/{id}/attachments{query}"),
            Some(&self.token),
            bytes,
        )
    }

    fn comment(&self, id: &str, text: &str) {
        let body = json!({"ops": [{"op": "comment", "id": id, "text": text}]});
        let reply = send(
            self.private(),
            "POST",
            "/pj/api/batch",
            Some(&self.token),
            body.to_string().as_bytes(),
        );
        assert_eq!(reply.status, 200, "{}", reply.text());
    }
}

#[test]
fn uploads_are_stored_once_renamed_on_a_clash_and_refused_when_not_images() {
    let hubs = Hubs::start();
    let id = hubs.create("Board looks wrong");
    let first = hubs.upload(
        &id,
        "?name=screenshot.png&caption=the+board",
        &png(2880, 1800, 1),
    );
    assert_eq!(first.status, 200, "{}", first.text());
    let first = first.json();
    assert_eq!(first["name"], "screenshot.png");
    assert_eq!(first["media_type"], "image/png");
    assert_eq!(
        (first["width"].as_u64(), first["height"].as_u64()),
        (Some(2880), Some(1800))
    );
    assert_eq!(first["caption"], "the board");
    // The same image sent again, as a client does after an unclear failure, is kept once.
    let again = hubs
        .upload(&id, "?name=screenshot.png", &png(2880, 1800, 1))
        .json();
    assert_eq!(again["name"], "screenshot.png");
    // Another image under the same name is renamed.
    let other = hubs
        .upload(&id, "?name=screenshot.png", &png(10, 10, 2))
        .json();
    assert_eq!(other["name"], "screenshot-2.png");
    // The same image on another ticket is stored once on disk.
    let elsewhere = hubs.create("Same board");
    assert_eq!(hubs.upload(&elsewhere, "", &png(2880, 1800, 1)).status, 200);
    let stored = std::fs::read_dir(hubs.dir.join("attachments"))
        .unwrap()
        .count();
    assert_eq!(stored, 2);

    let svg = b"<svg xmlns='http://www.w3.org/2000/svg'><script>alert(1)</script></svg>";
    assert_eq!(hubs.upload(&id, "?name=x.svg", svg).status, 415);
    assert_eq!(hubs.upload(&id, "", b"plain text").status, 415);
    let mut huge = png(1, 1, 3);
    huge.resize(usize::try_from(MAX_ATTACHMENT_BYTES).unwrap() + 1, 0);
    assert_eq!(hubs.upload(&id, "", &huge).status, 413);
    // The budget of 1 MiB is shared by open tickets: an image that would pass it is refused.
    let mut large = png(1, 1, 4);
    large.resize(1024 * 1024, 0);
    let full = hubs.upload(&id, "", &large);
    assert_eq!(full.status, 507, "{}", full.text());
    assert!(full.text().contains("attachment storage is full"));
    let unknown = hubs.upload("TNOPE", "", &png(1, 1, 5));
    assert_eq!(unknown.status, 404);
    let anonymous = send(
        hubs.private(),
        "POST",
        &format!("/pj/api/tickets/{id}/attachments"),
        None,
        &png(1, 1, 6),
    );
    assert_eq!(anonymous.status, 401);

    // The ticket's detail lists both, and the file comes back with its type.
    let detail = send(
        hubs.private(),
        "GET",
        &format!("/pj/api/tickets/{id}"),
        None,
        b"",
    )
    .json();
    assert_eq!(detail["attachments"].as_array().unwrap().len(), 2);
    let file = send(
        hubs.private(),
        "GET",
        &format!("/pj/api/tickets/{id}/attachments/screenshot.png"),
        None,
        b"",
    );
    assert_eq!(file.status, 200);
    assert_eq!(file.body, png(2880, 1800, 1));
    assert!(
        file.head.contains("Content-Type: image/png"),
        "{}",
        file.head
    );
    let export = send(
        hubs.private(),
        "GET",
        &format!("/pj/api/export?id={id}"),
        None,
        b"",
    )
    .text();
    assert!(export.contains("screenshot-2.png"), "{export}");
}

#[test]
fn the_private_page_shows_the_images_and_the_public_one_does_not() {
    let hubs = Hubs::start();
    let id = hubs.create("Board looks wrong");
    let shown = hubs
        .upload(&id, "?name=board.png", &png(800, 600, 1))
        .json();
    hubs.upload(&id, "?name=unembedded.png", &png(20, 10, 2));
    hubs.comment(
        &id,
        "![the board](attachment:board.png)\n\n![gone](attachment:nothing.png)\n\n\
         ![outside](https://example.com/x.png)",
    );
    let sha = shown["sha256"].as_str().unwrap();
    let path = format!("/{id}");

    let private = send(hubs.private(), "GET", &path, None, b"");
    assert_eq!(private.status, 200);
    let page = private.text();
    assert!(page.contains(&format!("<img src=\"/attachments/{sha}\" alt=\"the board\" loading=\"lazy\" width=\"800\" height=\"600\">")), "{page}");
    assert!(page.contains("[missing attachment: nothing.png]"));
    assert!(page.contains("<a href=\"https://example.com/x.png\">outside</a>"));
    assert!(!page.contains("src=\"https://example.com"));
    assert!(page.contains("Attachments"));
    assert!(page.contains("unembedded.png"));
    assert!(page.contains("attached"));

    let public = send(hubs.public(), "GET", &path, None, b"").text();
    assert!(public.contains("Image, visible on the private dashboard"));
    assert!(!public.contains("/attachments/"), "{public}");

    let file = send(
        hubs.private(),
        "GET",
        &format!("/attachments/{sha}"),
        None,
        b"",
    );
    assert_eq!(file.status, 200);
    assert!(file.head.contains("X-Content-Type-Options: nosniff"));
    assert!(file.head.contains("immutable"));
    assert_eq!(
        send(
            hubs.public(),
            "GET",
            &format!("/attachments/{sha}"),
            None,
            b""
        )
        .status,
        404
    );
    let api = format!("/pj/api/tickets/{id}/attachments/board.png");
    assert_eq!(send(hubs.public(), "GET", &api, None, b"").status, 404);
    assert_eq!(
        send(
            hubs.private(),
            "GET",
            &format!("/attachments/{}", "0".repeat(64)),
            None,
            b""
        )
        .status,
        404
    );
}

#[test]
fn json_jsonl_text_and_gzip_files_upload_and_download() {
    let hubs = Hubs::start();
    let id = hubs.create("Capture attachments");

    let json_bytes = br#"{"seed":42,"turn":3}"#;
    let uploaded_json = hubs.upload(&id, "?name=replay.json", json_bytes);
    assert_eq!(uploaded_json.status, 200, "{}", uploaded_json.text());
    let json_row = uploaded_json.json();
    assert_eq!(json_row["name"], "replay.json");
    assert_eq!(json_row["media_type"], "application/json");
    assert_eq!(
        (json_row["width"].as_u64(), json_row["height"].as_u64()),
        (Some(0), Some(0))
    );

    let lines_bytes = b"{\"a\":1}\n{\"b\":2}\n";
    let uploaded_lines = hubs.upload(&id, "?name=journal.jsonl", lines_bytes);
    assert_eq!(uploaded_lines.status, 200, "{}", uploaded_lines.text());
    assert_eq!(uploaded_lines.json()["media_type"], "application/x-ndjson");

    let text_bytes = b"root\n  child\n";
    let uploaded_text = hubs.upload(&id, "?name=tree.txt", text_bytes);
    assert_eq!(uploaded_text.status, 200, "{}", uploaded_text.text());
    assert_eq!(uploaded_text.json()["media_type"], "text/plain");

    // A minimal valid gzip stream (header + empty deflate block + crc32 + isize).
    let gzip_bytes: &[u8] = &[
        0x1F, 0x8B, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0x03, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let uploaded_gz = hubs.upload(&id, "?name=frames.jsonl.gz", gzip_bytes);
    assert_eq!(uploaded_gz.status, 200, "{}", uploaded_gz.text());
    let gz_row = uploaded_gz.json();
    assert_eq!(gz_row["name"], "frames.jsonl.gz");
    assert_eq!(gz_row["media_type"], "application/gzip");

    // Malformed JSON and HTML are still refused.
    assert_eq!(hubs.upload(&id, "?name=bad.json", b"not json").status, 415);
    assert_eq!(
        hubs.upload(&id, "?name=x.html", b"<html></html>").status,
        415
    );

    // Download JSON and gzip back and verify bytes, content type and nosniff header.
    let got_json = send(
        hubs.private(),
        "GET",
        &format!("/pj/api/tickets/{id}/attachments/replay.json"),
        None,
        b"",
    );
    assert_eq!(got_json.status, 200);
    assert_eq!(got_json.body, json_bytes);
    assert!(
        got_json.head.contains("Content-Type: application/json"),
        "{}",
        got_json.head
    );
    assert!(got_json.head.contains("X-Content-Type-Options: nosniff"));

    let got_gz = send(
        hubs.private(),
        "GET",
        &format!("/pj/api/tickets/{id}/attachments/frames.jsonl.gz"),
        None,
        b"",
    );
    assert_eq!(got_gz.status, 200);
    assert_eq!(got_gz.body, gzip_bytes);
    assert!(
        got_gz.head.contains("Content-Type: application/gzip"),
        "{}",
        got_gz.head
    );

    // The private dashboard lists non-image files without broken <img> tags.
    let page = send(hubs.private(), "GET", &format!("/{id}"), None, b"").text();
    assert!(page.contains("replay.json"), "{page}");
    assert!(page.contains("frames.jsonl.gz"), "{page}");
    assert!(!page.contains("width=\"0\""), "{page}");
}
