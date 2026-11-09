//! Local bead filing for reports submitted from a project checkout.

use std::ffi::{OsStr, OsString};
use std::path::Path;

use serde_json::json;

use crate::journal;

/// Queues a saved report for local filing. The saved report remains available if filing fails.
pub fn enqueue(root: &Path, id: &str, bundled: Option<(&'static str, &'static str, &'static str)>) {
    let (repo, uv, tool_path) = match (std::env::var_os("BERYL_FILE_BUGS"), bundled) {
        (Some(repo), _) => (repo, "uv", None),
        (None, Some((repo, uv, path))) => (OsString::from(repo), uv, Some(path)),
        (None, None) => return,
    };
    let root = root.to_owned();
    let id = id.to_owned();
    std::thread::spawn(move || {
        if let Err(error) = file(&repo, uv, tool_path, &root, &id) {
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

fn file(
    repo: &OsStr,
    uv: &str,
    tool_path: Option<&str>,
    root: &Path,
    id: &str,
) -> std::io::Result<()> {
    let root = std::path::absolute(root)?;
    let mut command = std::process::Command::new(uv);
    if let Some(path) = tool_path {
        command.env("PATH", path);
    }
    let output = command
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
