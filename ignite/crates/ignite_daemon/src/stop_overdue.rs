//! Ending a verb process that was asked to stop, by any path, and still runs a minute later.
//!
//! On 2026-11-19 on jarvis, `ignite status` showed a `check` under a tip commit "stopping" for
//! 5.6 hours while its verb process (`ignite internal verb <handle>`, started by an older
//! release) used most of a core and held its tree. `ignite stop` ends a verb process that
//! ignores its stop ([`crate::stop::StopVerb`]), and a restart into a new release does the same
//! for a landing it stops (see `server/outdated.rs`). A stop asked by `ignite close` or `ignite
//! release` (see [`crate::trees`]), or one the daemon read back from the local database after a
//! restart, only asked: nothing ended a process that never looked at it.
//!
//! The host (see [`crate::host`]) keeps every stop it sees asked in the `handle_stops` table,
//! with when it was asked, whichever path asked. [`end_overdue`] reads those rows, and for each
//! handle whose verb process is still running [`STOP_WAIT`] after the request, writes the handle's
//! row as `stopped` and ends the verb process with its process group, as `ignite stop` does. The
//! daemon's watch of the process then finds the row finished and hands the `stopped` reply to
//! whoever waits on the handle, and its host ends the activity and the commands the process left.
//! Writing the row first means the watch does not take the dead process for a crash and run the
//! verb again.
//!
//! A handle that refused the stop (a landing whose push has begun) has no `asked` row, so it is
//! left alone, and so is one whose process finished on its own.

use std::sync::Mutex;

use rusqlite::Connection;

use ignite_core::ids::Handle;
use ignite_core::protocol::client::{Exit, Reply};
use ignite_core::time::Timestamp;
use ignite_core::traits::Clock;
use ignite_store::local::{handle_stops, handles as handle_store};

use crate::handles::{finish_in_row, lock};
use crate::stop::{EndVerbProcess, STOP_WAIT};

/// Ends every verb process whose stop was asked at least `wait` ago and that still runs, and
/// returns what was ended, as lines for the daemon's log.
pub fn end_overdue(
    db: &Mutex<Connection>,
    clock: &dyn Clock,
    ender: &dyn EndVerbProcess,
    wait: std::time::Duration,
) -> Vec<String> {
    let now = clock.now();
    let asked = match handle_stops::asked(&lock(db)) {
        Ok(asked) => asked,
        Err(error) => return vec![format!("cannot read the stops that were asked: {error}")],
    };
    let mut log = Vec::new();
    for stop in asked {
        let overdue =
            Timestamp::parse(&stop.at).is_ok_and(|at| now.seconds_since(at) >= wait.as_secs_f64());
        let Ok(handle) = Handle::new(&stop.handle) else {
            continue;
        };
        if !overdue || stop.refused.is_some() {
            continue;
        }
        let row = handle_store::get(&lock(db), handle.as_str()).ok().flatten();
        // A verb on a thread of the daemon names no process and cannot be ended.
        let Some(row) = row.filter(|row| row.state == "running" && row.pid.is_some()) else {
            continue;
        };
        let reply = stopped_reply(&row.verb, &handle, wait.as_secs());
        match finish_in_row(&lock(db), &handle, &reply, true, now) {
            Ok(true) => {}
            // It finished by itself since the row was read.
            Ok(false) => continue,
            Err(error) => {
                log.push(format!(
                    "cannot end {handle}: its row was not written: {error}"
                ));
                continue;
            }
        }
        match ender.end(&handle) {
            Ok(Some(what)) => log.push(format!(
                "ended {what}: `ignite {}` ({handle}) had not stopped within {} s of being asked",
                row.verb,
                wait.as_secs()
            )),
            Ok(None) => {}
            Err(reason) => log.push(format!(
                "`ignite {}` ({handle}) did not stop and could not be ended: {reason}",
                row.verb
            )),
        }
    }
    log
}

/// The `stopped` reply of a verb that the daemon ended after `wait_s` seconds.
fn stopped_reply(verb: &str, handle: &Handle, wait_s: u64) -> Reply {
    Reply {
        exit: Exit::Conflict,
        text: format!(
            "stopped  `ignite {verb}` ({handle}) was asked to stop and had not within {wait_s} s, \
             so its verb process was ended"
        ),
        json: Some(serde_json::json!({ "handle": handle, "verb": verb, "stopped": true })),
        handle: None,
    }
}

/// How long a verb process has to stop before [`end_overdue`] ends it.
pub const WAIT: std::time::Duration = STOP_WAIT;

#[cfg(test)]
#[path = "stop_overdue_tests.rs"]
mod tests;
