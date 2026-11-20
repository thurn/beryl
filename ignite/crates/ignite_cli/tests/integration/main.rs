//! Every integration test of `ignite_cli`, in one test binary.
//!
//! Each file under `tests/` was once a test binary of its own, and a change to `ignite_daemon`
//! relinked all eight. macOS checks a newly linked executable the first time it runs, about 1.5
//! to 2 s each and one at a time, and cargo-nextest runs every test binary once to list its
//! tests. On 2026-11-19, under a load average of about 20, that check took about 20 s of an
//! express run's test step after a one-line daemon edit, as long as the tests themselves. One
//! binary is checked once.

mod adopt;
mod cli;
mod end_to_end;
mod restart;
mod supervise;
mod supervise_cleanup;
mod ticket_cli;
mod wrap;

/// A shell loop that waits until the file at `path` exists, checking ten times a second, and
/// gives up after ten minutes.
///
/// A test that failed or was cancelled before it created the file used to leave its command
/// waiting forever. On 2026-11-20 jarvis had five such shells from runs eleven hours earlier,
/// each starting `sleep` a hundred times a second: about half a core of process starts, which
/// the scheduler counted as load outside Ignite.
#[cfg(unix)]
fn wait_for_file(path: &str) -> String {
    format!("i=0; while [ ! -e '{path}' ] && [ $i -lt 6000 ]; do sleep 0.1; i=$((i+1)); done")
}
