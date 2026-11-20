//! Fleet metrics: each daemon runs the efficiency dashboard's ingest for its own machine and
//! posts the new rows to the hub, so that the hub's machine has every machine's model tokens and
//! dollars without anyone copying exports by hand (`docs/ignite.md`, "Records and metrics").
//!
//! The commands come from the `[metrics]` table of `ignite.toml`. A call to [`post_new_rows`]
//! does nothing until a file under one of the watched directories (the session transcripts) has
//! been added, has grown or has changed. Then it runs the ingest, runs the export of this
//! machine's rows since the day of the last export it sent, and posts the export's lines to
//! `POST /metrics/import` with the machine's ID, in parts of a few megabytes ([`CHUNK_BYTES`]).
//! A first export holds the machine's whole history, which can be hundreds of megabytes, more
//! than the hub reads in one request; reading it a part at a time also keeps the daemon's memory
//! small. The hub stores each row once, and the
//! efficiency dashboard on the hub's machine imports them, naming the machine by the first host name it saw
//! for that ID, so that renaming a machine does not split its history.
//!
//! The commands run in the `batch` class. They are bookkeeping that has to finish, and the
//! scheduler stops `idle` commands whenever a higher class waits for cores, which on a busy
//! machine is within seconds: an `idle` export never got to finish.
//!
//! What was sent is kept in a small JSON file, so a restarted daemon carries on from it. An
//! export overlaps the last one by up to a day, which costs nothing: importing a row twice
//! changes nothing. The file also records when the ingest last ran, and a restarted daemon's
//! first round waits until the loop's period has passed since then ([`first_round_in`]). Before,
//! every start ran the ingest and the export at once, which cost 13 to 17 s of CPU per restart
//! on a day of restarts every few minutes.

use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use ignite_core::config::Metrics;
use ignite_core::protocol::Class;
use ignite_core::protocol::hub::MetricsImport;
use ignite_core::time::Timestamp;
use ignite_core::traits::{CommandSpec, Error, Result};

use crate::server::Context;

#[path = "metrics_filter.rs"]
mod filter;
use filter::{hub_host, hub_keeps};

/// The most bytes of export lines one `POST /metrics/import` carries. The hub reads bodies up
/// to 100 MB, and the client gives a request 30 seconds; a few megabytes stay far inside both.
const CHUNK_BYTES: usize = 4 * 1024 * 1024;

/// What the last successful post left behind, kept in the state file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Sent {
    /// The watched files' stamp ([`stamp`]) when the last export was taken.
    stamp: String,
    /// The UTC date the last export was taken, which the next export starts from.
    since: Option<String>,
    /// When the ingest last began, whether or not its round finished.
    #[serde(default)]
    last_round_at: Option<Timestamp>,
    /// The UTC date the spend windows were last sent, which the next send starts from. It is
    /// kept apart from `since` so that a machine whose exports began before the spend command
    /// existed sends its whole history of windows once.
    #[serde(default)]
    spend_since: Option<String>,
}

/// How long a daemon that has just started waits before its first round, so that rounds stay
/// `period` apart across restarts: what is left of `period` since the ingest last began, as
/// recorded in `state`, or nothing if it never ran.
pub fn first_round_in(
    state: &Path,
    now: Timestamp,
    period: std::time::Duration,
) -> std::time::Duration {
    let Some(last) = load(state).last_round_at else {
        return std::time::Duration::ZERO;
    };
    let left = period.as_secs_f64() - now.seconds_since(last);
    std::time::Duration::from_secs_f64(left.clamp(0.0, period.as_secs_f64()))
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
    // Recorded before the ingest runs, so a round that fails or is cut off by a restart still
    // counts for the wait before the next start's first round.
    save(
        state,
        &Sent {
            last_round_at: Some(started),
            ..sent.clone()
        },
    )?;
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
    let count = post_export(ctx, &out, started, CHUNK_BYTES)?;
    if !metrics.findings.is_empty() {
        post_findings(ctx, metrics.findings.clone())?;
    }
    if !metrics.spend.is_empty() {
        let spend_out = state.with_extension("spend.jsonl");
        let spend_since = sent.spend_since.as_deref().unwrap_or("all");
        let args = metrics
            .spend
            .iter()
            .map(|arg| {
                arg.replace("{since}", spend_since)
                    .replace("{out}", &spend_out.to_string_lossy())
            })
            .collect();
        run(ctx, args)?;
        post_spend(ctx, &spend_out, CHUNK_BYTES)?;
        let _ = fs::remove_file(&spend_out);
    }
    save(
        state,
        &Sent {
            stamp: now,
            since: Some(started.day()),
            last_round_at: Some(started),
            spend_since: if metrics.spend.is_empty() {
                sent.spend_since.clone()
            } else {
                Some(started.day())
            },
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

/// Posts the spend windows at `path`, one JSON object per line, in parts of about
/// `chunk_bytes`, from the source `spend`. The hub keeps the latest post of each window, so
/// sending one again changes nothing.
fn post_spend(ctx: &Context, path: &Path, chunk_bytes: usize) -> Result<u32> {
    let reading = |error: std::io::Error| Error::Io(format!("reading {}: {error}", path.display()));
    let invalid = |error: serde_json::Error| Error::Invalid(format!("{}: {error}", path.display()));
    let post = |rows: Vec<serde_json::Value>| {
        ctx.hub.metrics(&MetricsImport {
            machine: ctx.machine.clone(),
            source: ignite_store::hub::ticket_spend::SOURCE.to_owned(),
            rows,
        })
    };
    let (mut count, mut rows, mut bytes) = (0u32, Vec::new(), 0usize);
    for line in BufReader::new(File::open(path).map_err(reading)?).lines() {
        let line = line.map_err(reading)?;
        if line.trim().is_empty() {
            continue;
        }
        bytes += line.len();
        rows.push(serde_json::from_str(&line).map_err(invalid)?);
        count = count.saturating_add(1);
        if bytes >= chunk_bytes {
            post(std::mem::take(&mut rows))?;
            bytes = 0;
        }
    }
    if !rows.is_empty() {
        post(rows)?;
    }
    Ok(count)
}

/// Runs one of the dashboard's commands from the main checkout in the `batch` class, which the
/// scheduler never stops for other work, and returns the file that holds its output.
fn run(ctx: &Context, args: Vec<String>) -> Result<PathBuf> {
    let name = args.join(" ");
    let result = ctx.launcher.run(&CommandSpec {
        args,
        cwd: ctx.repo.clone(),
        env: Vec::new(),
        class: Class::Batch,
        cores: 1,
        resources: Vec::new(),
        grows: false,
        tree: None,
        ticket: None,
        hang_limit_s: None,
        reserve: None,
    });
    match result.exit {
        Some(0) => Ok(result.output),
        exit => Err(Error::Io(format!(
            "`{name}` failed with exit {exit:?}; its output is in {}",
            result.output.display()
        ))),
    }
}

/// Posts the export at `path` in parts whose rows take about `chunk_bytes` of its lines each,
/// and returns how many rows it held. An export with no header is refused, and one with no rows
/// posts nothing. The file is read a line at a time, so only one part is held in memory.
///
/// Each part starts with the export's header, so that the hub's machine can import any part
/// on its own. The hub stores an identical line once, so each part's header is marked with
/// `part`: when `started` and its number in this export. Without the mark, every part after
/// the first, and every export on the same day as the last, would arrive with no header of its
/// own, and the dashboard would skip its rows if it read them apart from the first.
fn post_export(ctx: &Context, path: &Path, started: Timestamp, chunk_bytes: usize) -> Result<u32> {
    let reading = |error: std::io::Error| Error::Io(format!("reading {}: {error}", path.display()));
    let invalid = |error: serde_json::Error| Error::Invalid(format!("{}: {error}", path.display()));
    let mut lines = BufReader::new(File::open(path).map_err(reading)?)
        .lines()
        .filter(|line| line.as_ref().map_or(true, |line| !line.trim().is_empty()));
    let header: serde_json::Value = match lines.next() {
        Some(line) => serde_json::from_str(&line.map_err(reading)?).map_err(invalid)?,
        None => serde_json::Value::Null,
    };
    if header.get("export").is_none() {
        return Err(Error::Invalid(format!(
            "{} is not an efficiency export",
            path.display()
        )));
    }
    let source = header
        .get("host")
        .and_then(serde_json::Value::as_str)
        .filter(|host| !host.is_empty())
        .ok_or_else(|| Error::Invalid(format!("{} names no host", path.display())))?
        .to_owned();
    let on_hub_machine = hub_host(&ctx.config.hub.url).is_some_and(|hub| hub == source);
    let mut part = Part {
        ctx,
        header,
        source,
        started,
        number: 0,
    };
    let (mut count, mut rows, mut bytes) = (0u32, Vec::new(), 0usize);
    for line in lines {
        let line = line.map_err(reading)?;
        let row: serde_json::Value = serde_json::from_str(&line).map_err(invalid)?;
        if !hub_keeps(ignite_store::hub::metrics::kind_of(&row), on_hub_machine) {
            continue;
        }
        bytes += line.len();
        rows.push(row);
        count = count.saturating_add(1);
        if bytes >= chunk_bytes {
            part.post(std::mem::take(&mut rows))?;
            bytes = 0;
        }
    }
    if !rows.is_empty() {
        part.post(rows)?;
    }
    Ok(count)
}

/// The parts of one export as [`post_export`] posts them.
struct Part<'a> {
    ctx: &'a Context,
    header: serde_json::Value,
    source: String,
    started: Timestamp,
    /// The next part's number.
    number: u32,
}

impl Part<'_> {
    /// Posts `rows` after a copy of the header marked with this part's number.
    fn post(&mut self, rows: Vec<serde_json::Value>) -> Result<()> {
        let mut header = self.header.clone();
        if let Some(fields) = header.as_object_mut() {
            fields.insert(
                "part".to_owned(),
                serde_json::Value::String(format!("{} {}", self.started, self.number)),
            );
        }
        self.number += 1;
        let mut lines = Vec::with_capacity(rows.len() + 1);
        lines.push(header);
        lines.extend(rows);
        self.ctx.hub.metrics(&MetricsImport {
            machine: self.ctx.machine.clone(),
            source: self.source.clone(),
            rows: lines,
        })
    }
}

/// The watched directories, with a leading `~` made the home directory.
fn watched(metrics: &Metrics) -> Vec<PathBuf> {
    let home = ignite_core::config::home_dir(|name| std::env::var_os(name));
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
            let dir = ignite_core::test_files::temp_dir()
                .join(format!("ignite-metrics-{name}-{}", std::process::id()));
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
        assert!(runs.iter().all(|r| r.class == Class::Batch));
        let imports = s.hub.metrics_imports();
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].machine.as_str(), "m-7f3a");
        assert_eq!(imports[0].source, "multivac");
        assert_eq!(imports[0].rows.len(), 3);
        assert!(!out.exists(), "the export is removed once sent");
    }

    #[test]
    fn a_large_export_is_posted_in_parts_that_each_start_with_their_own_header() {
        let scratch = Scratch::new("parts");
        let s = setup(&scratch, true);
        write_export(&s, 10);
        let out = s.state.with_extension("export.jsonl");
        let row_bytes = fs::read_to_string(&out)
            .unwrap()
            .lines()
            .nth(1)
            .unwrap()
            .len();
        // Each part takes rows until they reach three rows' worth of bytes.
        let count = post_export(&s.ctx, &out, s.ctx.clock.now(), 3 * row_bytes).unwrap();
        assert_eq!(count, 10);

        let imports = s.hub.metrics_imports();
        let sizes: Vec<usize> = imports.iter().map(|import| import.rows.len() - 1).collect();
        assert_eq!(sizes, [3, 3, 3, 1]);
        let parts: Vec<&str> = imports
            .iter()
            .map(|import| {
                let header = &import.rows[0];
                assert_eq!(header["export"], 1);
                assert_eq!(header["host"], "multivac");
                assert_eq!(import.source, "multivac");
                header["part"].as_str().unwrap()
            })
            .collect();
        assert_eq!(
            parts,
            [
                "2026-11-17T14:12:00.000Z 0",
                "2026-11-17T14:12:00.000Z 1",
                "2026-11-17T14:12:00.000Z 2",
                "2026-11-17T14:12:00.000Z 3"
            ],
            "each part's header differs, so the hub stores every one"
        );
        let numbers: Vec<u64> = imports
            .iter()
            .flat_map(|import| import.rows[1..].iter())
            .map(|row| row["row"]["n"].as_u64().unwrap())
            .collect();
        assert_eq!(numbers, (0..10).collect::<Vec<u64>>());
    }

    #[test]
    fn a_file_that_is_not_an_export_posts_nothing() {
        let scratch = Scratch::new("not-export");
        let s = setup(&scratch, true);
        let out = s.state.with_extension("export.jsonl");
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        fs::write(&out, "{\"table\": \"requests\", \"row\": {}}\n").unwrap();
        assert!(matches!(
            post_export(&s.ctx, &out, s.ctx.clock.now(), CHUNK_BYTES),
            Err(Error::Invalid(_))
        ));
        assert!(s.hub.metrics_imports().is_empty());
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
                hung: false,
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
    fn spend_windows_are_posted_from_all_history_then_from_the_last_day_sent() {
        let scratch = Scratch::new("spend");
        let s = setup_with(
            &scratch,
            true,
            "spend = [\"dash\", \"spend\", \"--since\", \"{since}\", \"--out\", \"{out}\"]\n",
        );
        let windows = s.state.with_extension("spend.jsonl");
        for (round, name) in ["a.jsonl", "b.jsonl"].iter().enumerate() {
            grow_transcripts(&scratch, name);
            write_export(&s, 1);
            fs::write(&windows, "{\"ticket\": \"TA\"}\n\n{\"ticket\": \"TB\"}\n").unwrap();
            post_new_rows(&s.ctx, &s.state).unwrap();
            let runs = s.launcher.runs();
            let spend = runs.last().unwrap();
            let since = if round == 0 { "all" } else { "2026-11-17" };
            assert_eq!(&args(spend)[..4], ["dash", "spend", "--since", since]);
        }
        let imports = s.hub.metrics_imports();
        let posted: Vec<_> = imports.iter().filter(|i| i.source == "spend").collect();
        assert_eq!(posted.len(), 2);
        assert_eq!(posted[0].rows.len(), 2);
        assert!(!windows.exists(), "the windows file is removed once sent");
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
    fn a_restarted_daemon_waits_out_the_period_since_the_last_ingest() {
        let scratch = Scratch::new("first-round");
        let s = setup(&scratch, true);
        let period = std::time::Duration::from_secs(600);
        let now = s.ctx.clock.now();
        assert_eq!(
            first_round_in(&s.state, now, period),
            std::time::Duration::ZERO,
            "the ingest never ran"
        );

        grow_transcripts(&scratch, "a.jsonl");
        write_export(&s, 1);
        post_new_rows(&s.ctx, &s.state).unwrap();

        assert_eq!(first_round_in(&s.state, now, period), period);
        assert_eq!(
            first_round_in(&s.state, now.plus_seconds(240.0), period),
            std::time::Duration::from_secs(360)
        );
        assert_eq!(
            first_round_in(&s.state, now.plus_seconds(900.0), period),
            std::time::Duration::ZERO
        );
    }

    #[test]
    fn a_failed_ingest_still_counts_for_the_wait_before_the_next_round() {
        let scratch = Scratch::new("failed-round");
        let s = setup(&scratch, true);
        grow_transcripts(&scratch, "a.jsonl");
        s.launcher.script(
            &["dash", "ingest"],
            RunResult {
                exit: Some(1),
                output: PathBuf::from("/out"),
                usage: ignite_core::protocol::Usage::default(),
                hung: false,
            },
        );
        assert!(post_new_rows(&s.ctx, &s.state).is_err());
        let period = std::time::Duration::from_secs(600);
        assert_eq!(first_round_in(&s.state, s.ctx.clock.now(), period), period);
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
                hung: false,
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
