//! The `scanning` finding in `ignite doctor`: whether macOS scans each newly built program the
//! first time the daemon's commands run it.
//!
//! On macOS, `syspolicyd` and `XprotectService` check every executable the first time it runs,
//! unless the process responsible for starting it is listed under Developer Tools in System
//! Settings, Privacy & Security. Every build script, proc macro and test binary a build links
//! is new. On 2026-11-20, with Ignite's builds running on jarvis, `XprotectService` scanned about four
//! programs a second, `syspolicyd` and `XprotectService` together used about one core, and a
//! freshly compiled empty C program took 180 ms to start the first time and 5 ms the second.
//! The scheduler counts that CPU as load outside Ignite and grants fewer cores because of it.
//!
//! The check compiles a tiny program that no earlier run has seen, runs it twice and compares
//! the two start times. The daemon runs it, so it measures the daemon's own exemption, which is
//! what its commands inherit.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use super::Finding;

/// How much longer the first run may take than the second before the check warns. An exempt
/// program's first run costs a few milliseconds more for reading it from disk; a scanned one
/// costs over a hundred.
const SCANNED_AFTER: Duration = Duration::from_millis(60);

/// What to do about scanning.
const FIX: &str = "in System Settings, Privacy & Security, Developer Tools, add the daemon's \
                   executable (the target of `~/.config/ignite/bin/current`; run `spctl \
                   developer-mode enable-terminal` once if the list is not shown), then restart \
                   the daemon. Each release is signed ad hoc, so macOS knows it by its code \
                   hash and the grant lapses at the next release";

/// A string no earlier probe used: the process ID and the time in nanoseconds.
fn unique() -> Option<String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    Some(format!("{}-{}", std::process::id(), now.as_nanos()))
}

/// The start times of a fresh program's first and second runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Starts {
    pub first: Duration,
    pub second: Duration,
}

/// The `scanning` finding, or `None` on a system other than macOS, where nothing scans new
/// programs this way, or when the probe could not be built.
pub fn finding() -> Option<Finding> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    // The daemon's own scratch folder, deleted below, so not a test's.
    #[allow(clippy::disallowed_methods)]
    let dir = std::env::temp_dir().join(format!("ignite-doctor-scanning-{}", unique()?));
    let starts = probe(&dir);
    let _ = std::fs::remove_dir_all(&dir);
    Some(judge(starts?))
}

/// Compiles a program in `dir` that no earlier run has seen, and times its first two runs.
fn probe(dir: &Path) -> Option<Starts> {
    std::fs::create_dir_all(dir).ok()?;
    let source = dir.join("probe.c");
    let binary = dir.join("probe");
    // A unique string makes the program's code hash new, so no earlier scan applies to it.
    let unique = unique()?;
    std::fs::write(
        &source,
        format!(
            "const char *unique = \"{unique}\";\nint main(void) {{ return unique[0] == 0; }}\n"
        ),
    )
    .ok()?;
    let built = Command::new("cc")
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output()
        .ok()?;
    if !built.status.success() {
        return None;
    }
    let time = || {
        let start = Instant::now();
        Command::new(&binary)
            .status()
            .ok()
            .filter(std::process::ExitStatus::success)?;
        Some(start.elapsed())
    };
    let first = time()?;
    let second = time()?;
    Some(Starts { first, second })
}

/// The finding for a probe whose runs took `starts`.
pub(super) fn judge(starts: Starts) -> Finding {
    let ms = |d: Duration| d.as_millis();
    let measured = format!(
        "a newly built program took {} ms to start the first time and {} ms the second",
        ms(starts.first),
        ms(starts.second)
    );
    if starts.first.saturating_sub(starts.second) > SCANNED_AFTER {
        Finding::warning(
            "scanning",
            format!(
                "{measured}: macOS scans every program a build links before it first runs, \
                 which costs CPU the scheduler counts as load outside Ignite."
            ),
            FIX,
        )
    } else {
        Finding::ok(
            "scanning",
            format!("{measured}: macOS does not scan newly built programs."),
        )
    }
}
