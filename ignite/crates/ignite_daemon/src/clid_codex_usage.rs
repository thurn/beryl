//! Reads the Codex rollout identified by a finished turn's `thread.started` event.

use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};

use ignite_core::cli::codex::{Codex, read_session_usage};
use ignite_core::cli::{CliApp, TurnEvents};
use ignite_core::config::home_dir;
use ignite_core::time::Timestamp;

use super::Health;
use crate::server::Context;

impl Health {
    pub(super) fn record_codex_usage(
        &self,
        ctx: &Context,
        provider: &str,
        events: &TurnEvents,
        started: Option<Timestamp>,
    ) {
        if ctx.config.cli_dialect(provider) != Codex.name() {
            return;
        }
        let (Some(thread), Some(started)) = (events.transcript.as_deref(), started) else {
            return;
        };
        let root = std::env::var_os("CODEX_HOME")
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .or_else(|| home_dir(|key| std::env::var_os(key)).map(|home| home.join(".codex")));
        if let Some(file) = root.and_then(|root| rollout(&root.join("sessions"), thread))
            && let Some((at, samples)) = read_session_usage(BufReader::new(file), started)
            && let Err(error) = self.record_usage(ctx, provider, &samples, at)
        {
            eprintln!("ignite: Codex usage samples not stored: {error}");
        }
    }
}

/// A resumed thread keeps its original rollout date. Search date directories latest first.
fn rollout(sessions: &Path, thread: &str) -> Option<File> {
    let suffix = format!("-{thread}.jsonl");
    for year in directories(sessions) {
        for month in directories(&year) {
            for day in directories(&month) {
                let Ok(files) = fs::read_dir(day) else {
                    continue;
                };
                for file in files.flatten() {
                    if file
                        .file_name()
                        .to_str()
                        .is_some_and(|name| name.starts_with("rollout-") && name.ends_with(&suffix))
                    {
                        return File::open(file.path()).ok();
                    }
                }
            }
        }
    }
    None
}

fn directories(path: &Path) -> Vec<PathBuf> {
    let mut directories: Vec<_> = fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect();
    directories.sort_unstable_by(|a, b| b.cmp(a));
    directories
}
