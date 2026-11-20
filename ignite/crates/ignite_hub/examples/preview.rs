//! Serves the dashboard built from this tree on the loopback address, over a private copy of a
//! hub database, so a dashboard change can be seen and driven in a browser without touching the
//! live hub.
//!
//! ```text
//! cargo run -p ignite_hub --example preview -- [--repo <dir>] [--from <hub.db>] [--listen <addr>] [--refresh]
//! ```
//!
//! - `--repo`: the repository whose `ignite.toml` (and `[hub] wordmark`) to use; the current
//!   directory or its nearest ancestor with an `ignite.toml` by default.
//! - `--from`: the database to copy; `$IGNITE_HOME/hub/hub.db`, or `~/.config/ignite/hub/hub.db`,
//!   by default. It is copied with `VACUUM INTO`, which reads a consistent snapshot and never
//!   writes to the source, so the live hub keeps running undisturbed.
//! - `--listen`: the address to serve on, `127.0.0.1:7399` by default.
//! - `--refresh`: copy the database again. Otherwise the copy from an earlier run is reused,
//!   since a large hub database takes a while to copy.
//!
//! The copy also gets one ticket, "Preview: attachments", with an image embedded in its text,
//! one that no text embeds and one whose file was removed, so the ticket page shows all three.
//!
//! The copy lives in the system temporary directory. Edits made in the preview change only the
//! copy. The page assets are compiled into the binary, so after editing a file under `static/`,
//! stop the preview and run it again.

use std::path::{Path, PathBuf};

use ignite_core::config::Config;
use ignite_hub::server::HubServer;

fn main() {
    if let Err(message) = run() {
        eprintln!("preview: {message}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut repo = None;
    let mut from = None;
    let mut listen = "127.0.0.1:7399".to_owned();
    let mut refresh = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--repo" => repo = Some(PathBuf::from(value()?)),
            "--from" => from = Some(PathBuf::from(value()?)),
            "--listen" => listen = value()?,
            "--refresh" => refresh = true,
            _ => return Err(format!("unknown argument {arg}")),
        }
    }
    let repo = match repo {
        Some(repo) => repo,
        None => find_repo()?,
    };
    let config_path = repo.join("ignite.toml");
    let text = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("{}: {e}", config_path.display()))?;
    let config = Config::parse(&config_path.to_string_lossy(), &text).map_err(|e| e.to_string())?;
    let wordmark = config
        .hub
        .wordmark
        .as_ref()
        .and_then(|path| std::fs::read_to_string(repo.join(path)).ok());

    let from = match from {
        Some(from) => from,
        None => default_database()?,
    };
    // A program a person runs, which keeps its copy between runs, not a test's scratch folder.
    #[allow(clippy::disallowed_methods)]
    let dir = std::env::temp_dir().join("ignite-preview");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let database = dir.join("hub.db");
    if refresh || !database.is_file() {
        copy(&from, &database)?;
    }
    let budget = config.hub.attachment_budget_bytes();
    if let Some(id) = seed_attachments(&database, budget)? {
        println!("preview: attachments are shown on /{id}");
    }

    let running = HubServer {
        config,
        listen,
        database: database.clone(),
        wordmark,
    }
    .start()
    .map_err(|e| e.to_string())?;
    println!(
        "preview: serving {} at http://{}/",
        database.display(),
        running.address()
    );
    running.wait();
    Ok(())
}

/// The current directory or its nearest ancestor that holds an `ignite.toml`.
fn find_repo() -> Result<PathBuf, String> {
    let here = std::env::current_dir().map_err(|e| e.to_string())?;
    here.ancestors()
        .find(|dir| dir.join("ignite.toml").is_file())
        .map(Path::to_path_buf)
        .ok_or_else(|| "no ignite.toml here or above; pass --repo".to_owned())
}

/// The machine's hub database, where the daemon keeps it.
fn default_database() -> Result<PathBuf, String> {
    let home = match std::env::var_os("IGNITE_HOME") {
        Some(home) => PathBuf::from(home),
        None => PathBuf::from(std::env::var_os("HOME").ok_or("HOME is not set")?)
            .join(".config")
            .join("ignite"),
    };
    let database = home.join("hub").join("hub.db");
    if database.is_file() {
        Ok(database)
    } else {
        Err(format!(
            "{} does not exist (this machine runs no hub); pass --from",
            database.display()
        ))
    }
}

/// Copies `from` to `to` as a consistent snapshot, opening the source read-only.
fn copy(from: &Path, to: &Path) -> Result<(), String> {
    let _ = std::fs::remove_file(to);
    println!("preview: copying {} …", from.display());
    let source = rusqlite::Connection::open_with_flags(
        from,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("{}: {e}", from.display()))?;
    source
        .execute("VACUUM INTO ?1", [to.to_string_lossy()])
        .map_err(|e| format!("copying {}: {e}", from.display()))?;
    Ok(())
}

/// Files the ticket that shows each kind of attachment, unless the copy has it already, and
/// returns its ID.
fn seed_attachments(database: &Path, budget: u64) -> Result<Option<String>, String> {
    use ignite_core::protocol::attachments::sniff;
    use ignite_core::protocol::tickets::{BatchRequest, Operation, TicketFields};
    use ignite_core::time::Timestamp;
    use ignite_store::hub::{self, attachments, batch};

    const TITLE: &str = "Preview: attachments";
    let text = |e: &dyn std::fmt::Display| e.to_string();
    let mut conn = hub::open(database).map_err(|e| text(&e))?;
    let found: Option<String> = conn
        .query_row("SELECT id FROM tickets WHERE title = ?1", [TITLE], |row| {
            row.get(0)
        })
        .ok();
    if found.is_some() {
        return Ok(found);
    }
    let now = Timestamp::parse("2026-11-20T06:00:00Z").map_err(|e| text(&e))?;
    let body = "The board drew the attacker twice:\n\n![The board](attachment:board.png)\n\n\
                An earlier capture:\n\n![Earlier](attachment:old.png)";
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops: vec![Operation::Create {
            fields: TicketFields {
                title: Some(TITLE.to_owned()),
                body: Some(body.to_owned()),
                ..TicketFields::default()
            },
            parent: None,
            deps: Vec::new(),
            request_key: None,
        }],
    };
    let id = match batch::apply(&mut conn, &request, "preview", "preview", now)
        .map_err(|e| text(&e))?
    {
        batch::BatchOutcome::Applied(reply) => reply.results[0].id.clone(),
        batch::BatchOutcome::Conflict(_) => return Err("seeding the ticket conflicted".to_owned()),
    };
    let files = ignite_hub::attachments::AttachmentFiles::beside(database, budget);
    for (name, width, height, colour) in [
        ("board.png", 960, 540, [29, 122, 114]),
        ("notes.png", 320, 480, [217, 122, 38]),
        ("old.png", 640, 360, [107, 107, 114]),
    ] {
        let bytes = sample_png(width, height, colour);
        let sha256 = files.write(&bytes).map_err(|e| text(&e))?;
        let upload = attachments::Upload {
            ticket: &id,
            name,
            sha256: &sha256,
            image: sniff(&bytes).ok_or("the sample is not a PNG")?,
            bytes: bytes.len() as u64,
            caption: None,
        };
        attachments::insert(&mut conn, &upload, "preview", now).map_err(|e| text(&e))?;
    }
    conn.execute(
        "UPDATE attachments SET removed_at = ?2 WHERE ticket = ?1 AND name = 'old.png'",
        [id.as_str(), "2026-11-21T06:00:00Z"],
    )
    .map_err(|e| text(&e))?;
    Ok(Some(id.to_string()))
}

/// A PNG of one colour, its pixels in uncompressed deflate blocks, which needs no compressor.
fn sample_png(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &byte in bytes {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }
    fn chunk(out: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) {
        out.extend(u32::try_from(data.len()).unwrap_or(0).to_be_bytes());
        let start = out.len();
        out.extend(kind);
        out.extend(data);
        let crc = crc32(&out[start..]);
        out.extend(crc.to_be_bytes());
    }
    // Each row is a filter byte (none) and then the pixels.
    let row: Vec<u8> = std::iter::once(0)
        .chain((0..width).flat_map(|_| rgb))
        .collect();
    let raw: Vec<u8> = (0..height).flat_map(|_| row.iter().copied()).collect();
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + u32::from(byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    let mut zlib = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (index, block) in blocks.iter().enumerate() {
        zlib.push(u8::from(index + 1 == blocks.len()));
        let length = u16::try_from(block.len()).unwrap_or(u16::MAX);
        zlib.extend(length.to_le_bytes());
        zlib.extend((!length).to_le_bytes());
        zlib.extend(*block);
    }
    zlib.extend(((b << 16) | a).to_be_bytes());
    let mut header = Vec::new();
    header.extend(width.to_be_bytes());
    header.extend(height.to_be_bytes());
    header.extend([8, 2, 0, 0, 0]);
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut png, *b"IHDR", &header);
    chunk(&mut png, *b"IDAT", &zlib);
    chunk(&mut png, *b"IEND", &[]);
    png
}
