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
    let dir = std::env::temp_dir().join("ignite-preview");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let database = dir.join("hub.db");
    if refresh || !database.is_file() {
        copy(&from, &database)?;
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
