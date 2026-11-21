//! The notices `announce` steps show, as floats of the flat layer (docs/beryl-3d.md, "Verbs"
//! and "Floats"): the turn banner and the other banners across the middle of the window
//! (`floats::BANNER`), the pill above a player's life total that says what their scry did
//! (`floats::SCRY_PILL`), and a coin's flip (`floats::COIN`). The words are the web
//! interface's (`ui/src/stage/notices.ts` and `ui/src/game/TurnBanner.tsx`).
//!
//! - `NoticeFloats::prepare` lays out the element of every notice of the playing and waiting
//!   scripts as each update arrives, under the notice's key (`notice::key`), so that a frame
//!   lays nothing out.
//! - `NoticeFloats::write` shows in each float the notice under way there that began last, as
//!   the web's `announced` picks one, and sets its look: it fades in and out over the shares
//!   of its run its kind gives (`notice::Fade`).

use beryl_protocol::protocol::{PlayerView, Side};
use glam::Vec2;

use game_play::notice::{Announced, NoticeRun, Place};
use game_table::table::{HALF_LIFE, HudAnchor, Layout};
use kit_base::tokens::{ratio, space};
use kit_ui::element::{FloatId, floats};
use kit_ui::layout::{FloatLook, Layer};

/// Every float a notice is shown in, with the place that shows there.
const FLOATS: [(FloatId, Place); 4] = [
    (floats::BANNER, Place::Banner),
    (floats::SCRY_PILL[0], Place::Pill(Side::You)),
    (floats::SCRY_PILL[1], Place::Pill(Side::Opponent)),
    (floats::COIN, Place::Coin),
];

/// The floats that show notices, and the key each shows now.
#[derive(Debug, Default)]
pub struct NoticeFloats {
    shown: [Option<u64>; FLOATS.len()],
}

impl NoticeFloats {
    /// Lays out in `layer` the element of each notice in `coming` (`Show::coming_notices`),
    /// and forgets those laid out before that are neither coming nor in `held`
    /// (`Show::notices`). The client calls it as each update arrives, so that `write` lays
    /// nothing out and allocates nothing.
    pub fn prepare<'a>(
        layer: &mut Layer,
        coming: impl Iterator<Item = (usize, &'a [NoticeRun], &'a PlayerView)>,
        held: &[Announced],
    ) {
        let mut keys: Vec<u64> = held.iter().map(|announced| announced.key).collect();
        for (script, runs, view) in coming {
            for run in runs {
                let key = game_play::notice::key(script, run.step);
                keys.push(key);
                let place = Place::of(&run.notice);
                // A notice only tells what happened and takes no press, so a press on it
                // reaches the card beneath, which it may stand over while it shows.
                layer.prepare(float_of(place), key, || {
                    view::element(&run.notice, view)
                        .name(name_of(place))
                        .passes(true)
                });
            }
        }
        for (id, _) in FLOATS {
            layer.forget_prepared(id, |key| keys.contains(&key));
        }
    }

    /// Shows in `layer` the notices of `notices` under way at `now`, placed for `layout`, or none of them unless `visible`. Lays nothing out and allocates
    /// nothing for what `prepare` laid out.
    pub fn write(
        &mut self,
        layer: &mut Layer,
        notices: &[Announced],
        now: u64,
        layout: &Layout,
        visible: bool,
    ) {
        for (at, (id, place)) in FLOATS.into_iter().enumerate() {
            let showing = (notices.iter())
                .rev()
                .find(|announced| visible && announced.place == place && announced.under_way(now));
            let want = showing.map(|announced| announced.key);
            if want != self.shown[at] {
                let shown = layer.show_prepared(id, want);
                debug_assert!(shown, "nothing is prepared under {want:?} for float {id:?}");
                if !shown {
                    layer.show_prepared(id, None);
                }
                self.shown[at] = want.filter(|_| shown);
            }
            let Some(announced) = showing else {
                continue;
            };
            let slide = Vec2::Y * slide(announced, now);
            let anchor = match (place, layout.bands.is_some()) {
                // In a banded layout banners cross the middle, the coin sits above it,
                // and a scry's pill sits at the end of its player's lands.
                (Place::Coin, true) => {
                    HudAnchor::Middle.at(Vec2::new(0.5, 0.0), Vec2::new(0.5, 1.0), slide)
                }
                (Place::Pill(player), true) => {
                    let end = Vec2::new(1.0, 0.5);
                    HudAnchor::BackStrip(player).at(end, end, slide)
                }
                (Place::Banner | Place::Coin, _) => {
                    HudAnchor::Middle.at(Vec2::splat(0.5), Vec2::splat(0.5), slide)
                }
                (Place::Pill(player), false) => HudAnchor::Life(player).at(
                    Vec2::ZERO,
                    Vec2::new(0.5, 1.0),
                    slide - Vec2::Y * (HALF_LIFE + space::S),
                ),
            };
            layer.anchor_float(
                id,
                anchor,
                FloatLook {
                    opacity: opacity(announced, now),
                    scale: scale(announced, now),
                    ..FloatLook::default()
                },
            );
        }
    }
}

/// The float `place` is shown in.
fn float_of(place: Place) -> FloatId {
    FLOATS
        .iter()
        .find(|(_, shows)| *shows == place)
        .map_or(floats::BANNER, |(id, _)| *id)
}

/// What the tree calls the float `place` is shown in: each its own name, since two notices may
/// stand at once.
const fn name_of(place: Place) -> &'static str {
    match place {
        Place::Banner => "banner",
        Place::Pill(Side::You) => "your pill",
        Place::Pill(Side::Opponent) => "opponent's pill",
        Place::Coin => "coin",
    }
}

/// How opaque `announced` is at `now`: rising from 0 to 1 over the first `fade_in` of its run,
/// and falling back to 0 over the last `fade_out` (`notice::Fade`).
fn opacity(announced: &Announced, now: u64) -> f32 {
    let over = announced.over;
    if over.duration() == 0 {
        return 1.0;
    }
    #[expect(clippy::cast_precision_loss, reason = "a notice lasts seconds at most")]
    let through = (now.saturating_sub(over.from)) as f32 / over.duration() as f32;
    (through / announced.fade.fade_in)
        .min((1.0 - through) / announced.fade.fade_out)
        .clamp(0.0, 1.0)
}

/// How much larger than laid out `announced` is drawn at `now`: `Fade::scale_from` as it starts
/// to fade in, growing linearly to 1 as it finishes fading in.
fn scale(announced: &Announced, now: u64) -> f32 {
    let from = announced.fade.scale_from;
    let grown = (progress(announced, now) / announced.fade.fade_in).min(1.0);
    from + (1.0 - from) * grown
}

/// How far down, in points, `announced` has moved at `now`: none until `ratio::TURN_BANNER_SLIDE_AT` of its run, then
/// linearly to `Fade::slide`.
fn slide(announced: &Announced, now: u64) -> f32 {
    let start = ratio::TURN_BANNER_SLIDE_AT;
    announced.fade.slide * ((progress(announced, now) - start) / (1.0 - start)).clamp(0.0, 1.0)
}

/// How far through its run `announced` is at `now`, from 0 to 1.
#[expect(clippy::cast_precision_loss, reason = "a notice lasts seconds at most")]
fn progress(announced: &Announced, now: u64) -> f32 {
    let over = announced.over;
    if over.duration() == 0 {
        return 1.0;
    }
    (now.saturating_sub(over.from)) as f32 / over.duration() as f32
}

mod view;
pub use view::step_banner;
