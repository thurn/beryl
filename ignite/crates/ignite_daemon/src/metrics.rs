//! Fleet metrics: each daemon runs the efficiency dashboard's ingest for its own machine and
//! posts the new rows to the hub, so that the hub's machine has every machine's model tokens and
//! dollars without anyone copying exports by hand (`docs/ignite.md`, "Records and metrics").
//!
//! The commands come from the `[metrics]` table of `ignite.toml`. A call to [`post_new_rows`]
//! does nothing until a file under one of the watched directories (the session transcripts) has
//! been added, has grown or has changed. Then it runs the ingest, runs the export of this
//! machine's rows since the day of the last export it sent, and posts the export's lines to
//! `POST /metrics/import` with the machine's ID. The hub stores each row once, and the
//! efficiency dashboard on the hub's machine imports them, naming the machine by the first host name it saw
//! for that ID, so that renaming a machine does not split its history.
//!
//! What was sent is kept in a small JSON file, so a restarted daemon carries on from it. An
//! export overlaps the last one by up to a day, which costs nothing: importing a row twice
//! changes nothing.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use ignite_core::config::Metrics;
use ignite_core::protocol::Class;
use ignite_core::protocol::hub::MetricsImport;
use ignite_core::traits::{CommandSpec, Error, Result};

use crate::server::Context;

/// What the last successful post left behind, kept in the state file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Sent {
    /// The watched files' stamp ([`stamp`]) when the last export was taken.
    stamp: String,
    /// The UTC date the last export was taken, which the next export starts from.
    since: Option<String>,
}

/// Runs the ingest and posts its new rows if the watched files changed since the last post;
/// returns how many rows were posted. `state` is the file that records the last post; the
/// export is written beside it and removed once sent. When the hub cannot be reached, nothing
/// is recorded, so the next call tries again.
pub fn post_new_rows(ctx: &Context, state: &Path) -> Result<u32> {
    let Some(metrics) = &ctx.config.metrics else {
        return Ok(0);
    };
    let sent = load(state);
    let now = stamp(&watched(metrics));
    if now == sent.stamp {
        return Ok(0);
    }
    let started = ctx.clock.now();
    run(ctx, metrics.ingest.clone())?;
    let out = state.with_extension("export.jsonl");
    let since = sent.since.as_deref().unwrap_or("all");
    let export = metrics
        .export
        .iter()
        .map(|arg| {
            arg.replace("{since}", since)
                .replace("{out}", &out.to_string_lossy())
        })
        .collect();
    run(ctx, export)?;
    let rows = read_export(&out)?;
    let source = rows[0]
        .get("host")
        .and_then(serde_json::Value::as_str)
        .filter(|host| !host.is_empty())
        .ok_or_else(|| Error::Invalid(format!("{} names no host", out.display())))?
        .to_owned();
    let count = u32::try_from(rows.len() - 1).unwrap_or(u32::MAX);
    if count > 0 {
        ctx.hub.metrics(&MetricsImport {
            machine: ctx.machine.clone(),
            source,
            rows,
        })?;
    }
    if !metrics.findings.is_empty() {
        post_findings(ctx, metrics.findings.clone())?;
    }
    save(
        state,
        &Sent {
            stamp: now,
            since: Some(started.day()),
        },
    )?;
    let _ = fs::remove_file(&out);
    Ok(count)
}

/// Runs the findings command and posts its JSON report as one row from the source `findings`,
/// the source the hub's efficiency rules read. The report is whatever the command printed
/// last that parses as a JSON object, so a warning printed before it does not matter.
fn post_findings(ctx: &Context, args: Vec<String>) -> Result<()> {
    let output = run(ctx, args)?;
    let text = fs::read_to_string(&output)
        .map_err(|error| Error::Io(format!("reading {}: {error}", output.display())))?;
    let report = text
        .match_indices('{')
        .find_map(|(start, _)| serde_json::from_str::<serde_json::Value>(&text[start..]).ok())
        .ok_or_else(|| Error::Invalid(format!("{} holds no findings report", output.display())))?;
    ctx.hub.metrics(&MetricsImport {
        machine: ctx.machine.clone(),
        source: "findings".to_owned(),
        rows: vec![report],
    })?;
    Ok(())
}

/// Runs one of the dashboard's commands from the main checkout, as idle work, and returns the
/// file that holds its output.
fn run(ctx: &Context, args: Vec<String>) -> Result<PathBuf> {
    let name = args.join(" ");
    let result = ctx.launcher.run(&CommandSpec {
        args,
        cwd: ctx.repo.clone(),
        env: Vec::new(),
        class: Class::Idle,
        cores: 1,
        resources: Vec::new(),
        tree: None,
        ticket: None,
    });
    match result.exit {
        Some(0) => Ok(result.output),
        exit => Err(Error::Io(format!(
            "`{name}` failed with exit {exit:?}; its output is in {}",
            result.output.display()
        ))),
    }
}

/// The export's lines, header first. An export with no header is refused.
fn read_export(path: &Path) -> Result<Vec<serde_json::Value>> {
    let text = fs::read_to_string(path)
        .map_err(|error| Error::Io(format!("reading {}: {error}", path.display())))?;
    let rows = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str)
        .collect::<std::result::Result<Vec<serde_json::Value>, _>>()
        .map_err(|error| Error::Invalid(format!("{}: {error}", path.display())))?;
    match rows.first() {
        Some(header) if header.get("export").is_some() => Ok(rows),
        _ => Err(Error::Invalid(format!(
            "{} is not an efficiency export",
            path.display()
        ))),
    }
}

/// The watched directories, with a leading `~` made the home directory.
fn watched(metrics: &Metrics) -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    metrics
        .watch
        .iter()
        .map(|dir| match (dir.strip_prefix("~/"), &home) {
            (Some(rest), Some(home)) => home.join(rest),
            _ => PathBuf::from(dir),
        })
        .collect()
}

/// A stamp of every file under `dirs`: how many there are, their total size and the latest
/// modification time. It changes when a transcript is added or grows. A directory that does
/// not exist counts as empty.
fn stamp(dirs: &[PathBuf]) -> String {
    let (mut files, mut bytes, mut latest) = (0u64, 0u64, 0u128);
    let mut pending: Vec<PathBuf> = dirs.to_vec();
    while let Some(dir) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                pending.push(entry.path());
            } else if meta.is_file() {
                files += 1;
                bytes += meta.len();
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |since| since.as_nanos());
                latest = latest.max(modified);
            }
        }
    }
    format!("{files}:{bytes}:{latest}")
}

fn load(state: &Path) -> Sent {
    fs::read_to_string(state)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save(state: &Path, sent: &Sent) -> Result<()> {
    if let Some(dir) = state.parent() {
        fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string(sent).map_err(|error| Error::Invalid(error.to_string()))?;
    let temporary = state.with_extension("tmp");
    fs::write(&temporary, text)?;
    fs::rename(&temporary, state)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;
    use std::sync::Arc;

    use ignite_core::config::Config;
    use ignite_core::fakes::{FakeBuilds, FakeClock, FakeGit, FakeHub, FakeLauncher};
    use ignite_core::ids::MachineId;
    use ignite_core::time::Timestamp;
    use ignite_core::traits::RunResult;

    use super::*;

    /// A temporary directory for one test, removed when dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("ignite-metrics-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(dir.join("transcripts")).unwrap();
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    struct Setup {
        ctx: Context,
        hub: Arc<FakeHub>,
        launcher: Arc<FakeLauncher>,
        state: PathBuf,
    }

    fn setup(scratch: &Scratch, metrics: bool) -> Setup {
        setup_with(scratch, metrics, "")
    }

    fn setup_with(scratch: &Scratch, metrics: bool, extra: &str) -> Setup {
        let mut text = r#"
            [repository]
            remote = "origin"
            staging = "staging"
            master = "master"
            trees = "trees"
            checks = "checks"
            [hub]
            url = "http://localhost:1"
            project = "test"
            [gate]
            threshold = 0.01
            cap = 10
            [machine.local]
            ticket_trees = 1
            landing_trees = 1
        "#
        .to_owned();
        if metrics {
            let _ = write!(
                text,
                "[metrics]\nwatch = [{:?}]\ningest = [\"dash\", \"ingest\"]\n\
                 export = [\"dash\", \"export\", \"--since\", \"{{since}}\", \"--out\", \"{{out}}\"]\n",
                scratch.0.join("transcripts").to_string_lossy()
            );
            text.push_str(extra);
        }
        let hub = Arc::new(FakeHub::default());
        let launcher = Arc::new(FakeLauncher::default());
        let ctx = Context {
            config: Config::parse("ignite.toml", &text).unwrap(),
            repo: scratch.0.clone(),
            machine: MachineId::new("m-7f3a").unwrap(),
            clock: Arc::new(FakeClock::new(
                Timestamp::parse("2026-11-17T14:12:00Z").unwrap(),
            )),
            hub: hub.clone(),
            git: Arc::new(FakeGit::default()),
            launcher: launcher.clone(),
            builds: Arc::new(FakeBuilds::default()),
            session: None,
            tree: None,
        };
        Setup {
            ctx,
            hub,
            launcher,
            state: scratch.0.join("state").join("metrics.json"),
        }
    }

    /// Writes the export the fake launcher's `export` command would have written.
    fn write_export(setup: &Setup, rows: usize) {
        let mut text = String::from(r#"{"export": 1, "host": "multivac", "schema": 20}"#);
        text.push('\n');
        for n in 0..rows {
            let _ = writeln!(
                text,
                "{{\"table\": \"requests\", \"row\": {{\"host\": \"multivac\", \"n\": {n}}}}}"
            );
        }
        let out = setup.state.with_extension("export.jsonl");
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        fs::write(out, text).unwrap();
    }

    fn grow_transcripts(scratch: &Scratch, name: &str) {
        fs::write(scratch.0.join("transcripts").join(name), "{}\n").unwrap();
    }

    fn args(spec: &CommandSpec) -> Vec<&str> {
        spec.args.iter().map(String::as_str).collect()
    }

    #[test]
    fn new_transcripts_post_the_export_with_the_machine_id() {
        let scratch = Scratch::new("post");
        let s = setup(&scratch, true);
        grow_transcripts(&scratch, "a.jsonl");
        write_export(&s, 2);
        assert_eq!(post_new_rows(&s.ctx, &s.state).unwrap(), 2);
        let runs = s.launcher.runs();
        assert_eq!(args(&runs[0]), ["dash", "ingest"]);
        let out = s.state.with_extension("export.jsonl");
        assert_eq!(
            args(&runs[1]),
            [
                "dash",
                "export",
                "--since",
                "all",
                "--out",
                &*out.to_string_lossy()
            ]
        );
        assert!(runs.iter().all(|r| r.class == Class::Idle));
        let imports = s.hub.metrics_imports();
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].machine.as_str(), "m-7f3a");
        assert_eq!(imports[0].source, "multivac");
        assert_eq!(imports[0].rows.len(), 3);
        assert!(!out.exists(), "the export is removed once sent");
    }

    #[test]
    fn the_findings_report_is_posted_after_the_export() {
        let scratch = Scratch::new("findings");
        let s = setup_with(
            &scratch,
            true,
            "findings = [\"dash\", \"findings\", \"--json\"]\n",
        );
        grow_transcripts(&scratch, "a.jsonl");
        write_export(&s, 1);
        let report = scratch.0.join("findings.out");
        fs::write(
            &report,
            "warning: stale\n{\"groups\": [{\"findings\": []}]}\n",
        )
        .unwrap();
        s.launcher.script(
            &["dash", "findings"],
            RunResult {
                exit: Some(0),
                output: report,
                usage: ignite_core::protocol::Usage::default(),
            },
        );
        post_new_rows(&s.ctx, &s.state).unwrap();
        let runs = s.launcher.runs();
        assert_eq!(args(&runs[2]), ["dash", "findings", "--json"]);
        let imports = s.hub.metrics_imports();
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].source, "multivac");
        assert_eq!(imports[1].source, "findings");
        assert_eq!(imports[1].machine.as_str(), "m-7f3a");
        assert_eq!(imports[1].rows.len(), 1);
        assert!(imports[1].rows[0].get("groups").is_some());
    }

    #[test]
    fn nothing_runs_until_the_transcripts_change_and_then_from_the_last_day_sent() {
        let scratch = Scratch::new("again");
        let s = setup(&scratch, true);
        grow_transcripts(&scratch, "a.jsonl");
        write_export(&s, 1);
        post_new_rows(&s.ctx, &s.state).unwrap();
        assert_eq!(post_new_rows(&s.ctx, &s.state).unwrap(), 0);
        assert_eq!(
            s.launcher.runs().len(),
            2,
            "unchanged transcripts run nothing"
        );

        grow_transcripts(&scratch, "b.jsonl");
        write_export(&s, 1);
        assert_eq!(post_new_rows(&s.ctx, &s.state).unwrap(), 1);
        let runs = s.launcher.runs();
        assert_eq!(&runs[3].args[2..4], ["--since", "2026-11-17"]);
        assert_eq!(s.hub.metrics_imports().len(), 2);
    }

    #[test]
    fn an_unreachable_hub_leaves_the_rows_for_the_next_call() {
        let scratch = Scratch::new("down");
        let s = setup(&scratch, true);
        grow_transcripts(&scratch, "a.jsonl");
        write_export(&s, 1);
        s.hub.set_unreachable(true);
        assert!(matches!(
            post_new_rows(&s.ctx, &s.state),
            Err(Error::Unreachable(_))
        ));
        s.hub.set_unreachable(false);
        write_export(&s, 1);
        assert_eq!(post_new_rows(&s.ctx, &s.state).unwrap(), 1);
        assert_eq!(s.hub.metrics_imports().len(), 1);
    }

    #[test]
    fn a_failed_ingest_posts_nothing() {
        let scratch = Scratch::new("fail");
        let s = setup(&scratch, true);
        grow_transcripts(&scratch, "a.jsonl");
        s.launcher.script(
            &["dash", "ingest"],
            RunResult {
                exit: Some(1),
                output: PathBuf::from("/out"),
                usage: ignite_core::protocol::Usage::default(),
            },
        );
        assert!(post_new_rows(&s.ctx, &s.state).is_err());
        assert!(s.hub.metrics_imports().is_empty());
    }

    #[test]
    fn without_a_metrics_table_nothing_runs() {
        let scratch = Scratch::new("none");
        let s = setup(&scratch, false);
        grow_transcripts(&scratch, "a.jsonl");
        assert_eq!(post_new_rows(&s.ctx, &s.state).unwrap(), 0);
        assert!(s.launcher.runs().is_empty());
    }
}
