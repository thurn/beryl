//! Stub. The ticket "CLI mode: holds, probe turns, plan-limit release and usage samples" owns
//! this file and replaces the `todo!()` body.
#![allow(unused_variables)]

use ignite_core::cli::Hold;
use ignite_core::time::Timestamp;

/// Seconds between probe turns while an app is on a CLI hold.
pub const PROBE_EVERY_S: f64 = 300.0;

/// Whether a probe turn for the app on `hold` is due at `now`, given when the last one ran.
pub fn probe_due(hold: &Hold, last_probe: Option<Timestamp>, now: Timestamp) -> bool {
    todo!()
}
