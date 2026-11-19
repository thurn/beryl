//! What a family's commands compiled, and how long each fresh listing took.
//!
//! Cargo's compiler wrapper and target runner (`crate::wrap`) append one line per compile unit
//! and test process to the units file a command's environment names. Each line carries when the
//! unit started and ended, so the daemon can tell how much of a `list` command's wall time went
//! to compiling.
//!
//! A `cargo nextest list` compiles whatever its family's product build did not, and it waits
//! for Cargo's lock on the target directory while another Cargo command in the same tree holds
//! it. Neither shows in the `build` line, so each listing records both, and the `listed` line of
//! `ignite test`, `ignite check` and the landing gate reports the slowest families (see "The
//! threshold and the gate's budget" in `docs/ignite.md`).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use ignite_core::protocol::hub::CompileUnit;
use ignite_core::traits::RunResult;
use serde::Deserialize;

use super::Checks;

/// What Cargo prints while it waits for another Cargo command to release a lock, on the build
/// directory or on the package cache. `--cargo-quiet` hides it.
const LOCK_WAIT: &str = "Blocking waiting for file lock";

/// When a command's first compile started and its last compile ended, in milliseconds since
/// the Unix epoch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompileSpan {
    pub first_started_ms: i64,
    pub last_ended_ms: i64,
    /// Distinct crates compiled.
    pub crates: usize,
}

/// One fresh listing of a family: how long its `list` command ran, and how much of that went to
/// compiling and to waiting for Cargo's lock. A family listed more than once in a run (a failed
/// try, or a kept listing whose sources had moved) has one entry per listing, its tries summed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ListingTime {
    pub family: String,
    /// Seconds the `list` command ran, waiting for cores excluded.
    pub wall_s: f64,
    /// Distinct crates the command compiled.
    pub compiled: usize,
    /// Seconds from the first compile's start to the last compile's end.
    pub compile_s: f64,
    /// Seconds before compiling started, or until the command ended when it compiled nothing,
    /// counted only when Cargo said it was waiting for a lock.
    pub lock_s: f64,
}

impl ListingTime {
    /// Adds one try of the family's `list` command, which ended at `ended_ms`.
    pub(super) fn add_try(&mut self, result: &RunResult, span: Option<CompileSpan>, ended_ms: i64) {
        let wall_s = result.usage.wall_s.max(0.0);
        self.wall_s += wall_s;
        let started_ms = ended_ms - seconds_to_ms(wall_s);
        if let Some(span) = span {
            self.compiled += span.crates;
            self.compile_s += ms_to_seconds(span.last_ended_ms - span.first_started_ms).min(wall_s);
        }
        let waited = fs::read_to_string(&result.output).is_ok_and(|text| text.contains(LOCK_WAIT));
        if waited {
            let until_ms = span.map_or(ended_ms, |s| s.first_started_ms);
            self.lock_s += ms_to_seconds(until_ms - started_ms).min(wall_s);
        }
    }
}

/// Milliseconds as seconds, never negative.
fn ms_to_seconds(ms: i64) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let seconds = ms.max(0) as f64 / 1000.0;
    seconds
}

fn seconds_to_ms(seconds: f64) -> i64 {
    #[allow(clippy::cast_possible_truncation)]
    let ms = (seconds * 1000.0).round() as i64;
    ms
}

/// The start and end a unit line carries beside its [`CompileUnit`] fields; lines written by an
/// older wrapper have neither.
#[derive(Deserialize)]
struct UnitTimes {
    mode: String,
    #[serde(rename = "crate")]
    crate_name: String,
    started_ms: Option<i64>,
    ended_ms: Option<i64>,
}

/// The units in a units file's text, and the span of its compiles. Test processes, which the
/// runner records with [`crate::wrap::RUN_MODE`], compile nothing and are left out of the span.
pub(super) fn parse(text: &str) -> (Vec<CompileUnit>, Option<CompileSpan>) {
    let units = text
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let mut span: Option<CompileSpan> = None;
    let mut crates = std::collections::BTreeSet::new();
    for times in text
        .lines()
        .filter_map(|line| serde_json::from_str::<UnitTimes>(line).ok())
        .filter(|times| times.mode != crate::wrap::RUN_MODE)
    {
        crates.insert(times.crate_name);
        let (Some(started), Some(ended)) = (times.started_ms, times.ended_ms) else {
            continue;
        };
        let span = span.get_or_insert(CompileSpan {
            first_started_ms: started,
            last_ended_ms: ended,
            crates: 0,
        });
        span.first_started_ms = span.first_started_ms.min(started);
        span.last_ended_ms = span.last_ended_ms.max(ended);
    }
    if let Some(span) = &mut span {
        span.crates = crates.len();
    }
    (units, span)
}

/// Reads and deletes a command's units file.
pub(super) fn take(file: &Path) -> (Vec<CompileUnit>, Option<CompileSpan>) {
    let text = fs::read_to_string(file).unwrap_or_default();
    let _ = fs::remove_file(file);
    parse(&text)
}

/// The units file a command's environment names, if it names one.
pub(super) fn env_units_file(env: &[(String, String)]) -> Option<PathBuf> {
    env.iter()
        .find(|(name, _)| name == crate::wrap::UNITS_ENV)
        .map(|(_, file)| PathBuf::from(file))
}

impl Checks {
    /// The variables that make Cargo's target runner (and the compiler wrapper) record each test
    /// process (and compile) of a cargo-nextest command into a new units file, as the daemon's
    /// own builds do (`crate::wrap::cargo_env`). [`Checks::launch`] reads the file back and
    /// attaches its units to the command's record. Empty when this daemon has no `ignite`
    /// executable to point the scripts at, or cannot write them: the command then goes
    /// unrecorded unit by unit but is otherwise unchanged.
    pub(super) fn unit_recording_env(&self) -> Vec<(String, String)> {
        if self.settings.ignite_exe.as_os_str().is_empty() {
            return Vec::new();
        }
        if let Err(error) =
            crate::wrap::write_shims(&self.settings.shims, &self.settings.ignite_exe)
                .and_then(|()| fs::create_dir_all(&self.settings.outputs))
        {
            eprintln!(
                "ignite: cannot write the test runner to {}: {error}",
                self.settings.shims.display()
            );
            return Vec::new();
        }
        let units = self.settings.outputs.join(format!(
            "units-{}-{}.jsonl",
            std::process::id(),
            self.next_file.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_file(&units);
        crate::wrap::cargo_env(&self.settings.shims, &units)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_span_covers_compiles_and_leaves_out_test_processes() {
        let text = concat!(
            r#"{"crate":"a","mode":"build","instructions":null,"cpu_s":1.0,"rebuilt":true,"started_ms":1000,"ended_ms":4000}"#,
            "\n",
            r#"{"crate":"b","mode":"test","instructions":null,"cpu_s":1.0,"rebuilt":false,"started_ms":2000,"ended_ms":9000}"#,
            "\n",
            r#"{"crate":"b","mode":"run","instructions":null,"cpu_s":1.0,"rebuilt":false,"started_ms":9500,"ended_ms":20000}"#,
            "\n",
            r#"{"crate":"c","mode":"build","instructions":null,"cpu_s":1.0,"rebuilt":false}"#,
            "\n",
        );
        let (units, span) = parse(text);
        assert_eq!(units.len(), 4);
        assert_eq!(
            span,
            Some(CompileSpan {
                first_started_ms: 1000,
                last_ended_ms: 9000,
                crates: 3,
            })
        );
    }

    fn result(wall_s: f64, output: &str, name: &str) -> RunResult {
        let dir = std::env::temp_dir().join(format!("ignite-units-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, output).unwrap();
        RunResult {
            exit: Some(0),
            output: path,
            usage: ignite_core::protocol::Usage {
                wall_s,
                ..Default::default()
            },
            hung: false,
        }
    }

    #[test]
    fn a_lock_wait_lasts_until_the_first_compile() {
        let mut time = ListingTime::default();
        let ran = result(
            100.0,
            "Blocking waiting for file lock on build directory\n",
            "lock",
        );
        let span = CompileSpan {
            first_started_ms: 60_000,
            last_ended_ms: 99_000,
            crates: 12,
        };
        time.add_try(&ran, Some(span), 100_000);
        assert_eq!(time.compiled, 12);
        assert!((time.lock_s - 60.0).abs() < 1e-9, "{time:?}");
        assert!((time.compile_s - 39.0).abs() < 1e-9, "{time:?}");
    }

    #[test]
    fn without_cargo_saying_so_nothing_counts_as_a_lock_wait() {
        let mut time = ListingTime::default();
        time.add_try(&result(30.0, "", "quiet"), None, 30_000);
        assert!((time.wall_s - 30.0).abs() < 1e-9);
        assert!(time.lock_s.abs() < 1e-9);
        assert_eq!(time.compiled, 0);
    }
}
