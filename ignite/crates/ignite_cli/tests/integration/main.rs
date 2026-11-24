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
mod fixture;
mod restart;
mod supervise;
mod supervise_cleanup;
mod ticket_cli;
mod wrap;
