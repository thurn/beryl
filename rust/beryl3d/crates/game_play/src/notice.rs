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
use kit_base::data::presentation::Notices;
use kit_base::tokens::{ratio, size};

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

/// The shares of a notice's run over which it fades in and out, as the web stage's
/// `.stage-notice` rules in `ui/src/stage/stage.css` have them for its kind: the turn banner,
/// the pill of a scry, and the other banners (a coin's among them). The values are in
/// `data/beryl3d/notices.json`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fade {
    pub fade_in: f32,
    pub fade_out: f32,
    /// How large it is drawn as it starts to fade in, as a multiple of its size, growing
    /// linearly to its size as it finishes fading in.
    pub scale_from: f32,
    /// How far down it has moved, in points, when its run ends; it starts to move at `ratio::TURN_BANNER_SLIDE_AT` of its
    /// run, and moves linearly.
    pub slide: f32,
}

impl Fade {
    /// How `notice` fades.
    #[must_use]
    pub const fn of(notice: &Notice, tuning: Notices) -> Self {
        let shares = match notice {
            Notice::Turn { .. } => tuning.turn,
            Notice::Scry { .. } | Notice::TopOrBottom { .. } => tuning.pill,
            _ => tuning.banner,
        };
        // The turn banner (`.turn-banner.stage-notice` in `ui/src/stage/stage.css`) starts at
        // 0.93 of its size, and sinks 30 points toward the player whose turn it is at its end:
        // up the table for the opponent's, down for ours.
        let (scale_from, slide) = match notice {
            Notice::Turn { active: Side::You } => {
                (ratio::TURN_BANNER_START_SCALE, size::TURN_BANNER_SLIDE)
            }
            Notice::Turn {
                active: Side::Opponent,
            } => (ratio::TURN_BANNER_START_SCALE, -size::TURN_BANNER_SLIDE),
            _ => (1.0, 0.0),
        };
        Self {
            fade_in: shares.r#in,
            fade_out: shares.out,
            scale_from,
            slide,
        }
    }
}

/// A notice the show holds, in nanoseconds since the client started, until it ends: where it is
/// shown, how it fades, the key its element is laid out under (`key`), when it is shown and whether the table
/// plays on under it. It holds no words, so that keeping one allocates nothing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Announced {
    pub place: Place,
    pub fade: Fade,
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
