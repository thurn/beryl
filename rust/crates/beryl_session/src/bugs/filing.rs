//! Opt-in filing shared by the desktop app and the local spec server.

use std::ffi::OsStr;
use std::path::Path;

use serde_json::json;

use crate::journal;

/// Queues a saved report for local filing. The saved report remains available if filing fails.
pub fn enqueue(root: &Path, id: &str) {
    let Some(repo) = std::env::var_os("BERYL_FILE_BUGS") else {
        return;
    };
    let root = root.to_owned();
    let id = id.to_owned();
    std::thread::spawn(move || {
        if let Err(error) = file(&repo, &root, &id) {
            journal::warn(
                "bugs",
                format!("local filing failed: {error}"),
                json!({ "id": id }),
            );
            // The CLI spec server has no journal open by default.
            if journal::path().is_none() {
                eprintln!("bugs: {id}: local filing failed: {error}");
            }
        }
    });
}

fn file(repo: &OsStr, root: &Path, id: &str) -> std::io::Result<()> {
    let root = std::path::absolute(root)?;
    let output = std::process::Command::new("uv")
        .current_dir(repo)
        .env("BERYL_BUGS", root)
        .args([
            "run",
            "--quiet",
            "--no-sync",
            "--project",
            "tools",
            "by",
            "bugs",
            "file",
            "--",
            id,
        ])
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim(),
        ));
    }
    Ok(())
}
