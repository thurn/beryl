//! `ignite manifest lint`: checks that every input pattern in the check manifests matches a
//! tracked file and that every `crate:` names a workspace package. It runs locally, without the
//! daemon or the hub.

use std::path::Path;
use std::process::Command;

use ignite_core::config::Config;
use ignite_core::inputs::{self, CrateGraph};
use ignite_core::manifest::Manifest;
use ignite_core::protocol::client::{Exit, Reply};

use crate::args::ManifestCommand;

/// The configuration file at the checkout's root.
const CONFIG_FILE: &str = "ignite.toml";

pub fn run(command: &ManifestCommand, dir: &Path) -> Reply {
    match command {
        ManifestCommand::Lint => {
            lint(dir).unwrap_or_else(|message| Reply::text(Exit::Usage, message))
        }
    }
}

fn lint(dir: &Path) -> Result<Reply, String> {
    let root = ignite_git::tracked::toplevel(dir).map_err(|e| e.to_string())?;
    let config_path = root.join(CONFIG_FILE);
    let text = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("{}: {e}", config_path.display()))?;
    let config = Config::parse(CONFIG_FILE, &text).map_err(|e| e.to_string())?;
    let manifest = read_manifest(&root.join(&config.repository.checks))?;
    let tracked = ignite_git::tracked::files(&root).map_err(|e| e.to_string())?;
    let graph = crate_graph(&root, &tracked)?;
    let findings = inputs::lint(&manifest, &graph, &tracked);
    let lines: Vec<String> = findings.iter().map(ToString::to_string).collect();
    let json = serde_json::json!({ "findings": lines });
    let (exit, text) = if lines.is_empty() {
        (Exit::Success, "every input pattern matches".to_owned())
    } else {
        (Exit::Usage, lines.join("\n"))
    };
    let mut reply = Reply::text(exit, text);
    reply.json = Some(json);
    Ok(reply)
}

/// Every `*.toml` file in the manifest directory, in name order.
fn read_manifest(dir: &Path) -> Result<Manifest, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    paths.sort();
    let mut files = Vec::new();
    for path in paths {
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        files.push((path, text));
    }
    Manifest::from_files(&files).map_err(|e| e.to_string())
}

/// The packages of every Cargo workspace in the checkout: each tracked lock file marks one.
fn crate_graph(root: &Path, tracked: &[String]) -> Result<CrateGraph, String> {
    let mut graph = CrateGraph::default();
    let root_text = root.to_string_lossy();
    for lock in tracked
        .iter()
        .filter(|p| p.rsplit('/').next() == Some("Cargo.lock"))
    {
        let manifest_path = root.join(lock).with_file_name("Cargo.toml");
        let output = Command::new("cargo")
            .args([
                "metadata",
                "--format-version",
                "1",
                "--no-deps",
                "--manifest-path",
            ])
            .arg(&manifest_path)
            .current_dir(root)
            .output()
            .map_err(|e| format!("cargo metadata: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "cargo metadata for {}: {}",
                manifest_path.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let json = String::from_utf8_lossy(&output.stdout);
        graph
            .add_workspace(&json, &root_text)
            .map_err(|e| e.to_string())?;
    }
    Ok(graph)
}
