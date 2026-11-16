//! What an `announce` step shows: a banner or a pill, from when it begins to when it goes
//! (docs/beryl-3d.md, "Verbs").
//!
//! - The verb writes a `NoticeRun` on the script's clock when the script is compiled.
//! - As the script starts, the show turns each of its runs into an `Announced`, in real time,
//!   and keeps it until it ends, so that a notice whose step `passes` outlives its script, as
//!   the web stage's `Overlay::notices` does (`ui/src/stage/overlay.ts`).
//! - `hud::notices` shows each one under way as a float of the flat layer.

use beryl_protocol::protocol::Side;
use beryl_protocol::script::Notice;

use crate::verbs::Interval;

/// A notice shown over `over`, on the script's clock. `passes` says whether the table goes on
/// playing under it. `step` is the number of the step that wrote it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoticeRun {
    pub notice: Notice,
    pub over: Interval,
    pub passes: bool,
    pub step: usize,
}

/// Where a notice is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// Across the middle of the window: the turn banner, and the banner of a roll, a shuffle,
    /// a rewind, a phase, a manual turn or a game's start.
    Banner,
    /// In the pill above the player's life total: what a scry did, or the end of their library
    /// the opponent chose for a card.
    Pill(Side),
    /// Across the middle of the window, in a float of its own: a coin's flip.
    Coin,
}

impl Place {
    /// Where `notice` is shown.
    #[must_use]
    pub const fn of(notice: &Notice) -> Self {
        match notice {
            Notice::Scry { player, .. } | Notice::TopOrBottom { player, .. } => Self::Pill(*player),
            Notice::Coin { .. } => Self::Coin,
            Notice::Turn { .. }
            | Notice::Roll { .. }
            | Notice::Shuffle
            | Notice::Rewind { .. }
            | Notice::Phase { .. }
            | Notice::TableTurn { .. }
            | Notice::Game { .. } => Self::Banner,
        }
    }
}

/// A notice the show holds, in nanoseconds since the client started, until it ends: where it is
/// shown, the key its element is laid out under (`key`), when it is shown and whether the table
/// plays on under it. It holds no words, so that keeping one allocates nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Announced {
    pub place: Place,
    pub key: u64,
    pub over: Interval,
    pub passes: bool,
}

impl Announced {
    /// Whether it is shown at `now`: from its start until, not at, its end, as on the web stage.
    #[must_use]
    pub const fn under_way(&self, now: u64) -> bool {
        self.over.from <= now && now < self.over.to
    }
}

/// The key the element of the notice written by step `step` of the show's script numbered
/// `script` is laid out under: one for each notice the show is given.
#[must_use]
pub fn key(script: usize, step: usize) -> u64 {
    let script = u64::try_from(script).unwrap_or(u64::MAX);
    let step = u64::try_from(step).unwrap_or(0);
    (script << 20) | (step & 0xF_FFFF)
}
