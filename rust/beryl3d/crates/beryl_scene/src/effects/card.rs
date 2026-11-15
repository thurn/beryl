//! How the effects on a card change the way it is drawn. Not built yet: it changes nothing.

use beryl_protocol::script::Subject;

use super::EffectRun;
use crate::table::Look;

/// Changes `look`, how the card `on` is drawn at `now`, for the runs in `runs` that play on it.
pub fn look(_runs: &[EffectRun], _on: Subject, _now: u64, _look: &mut Look) {}
