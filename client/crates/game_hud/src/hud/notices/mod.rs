//! The notices `announce` steps show, as floats of the flat layer (docs/beryl-3d.md, "Verbs"
//! and "Floats"): the turn banner and the other banners across the middle of the window
//! (`floats::BANNER`), the pill above a player's life total that says what their scry did
//! (`floats::SCRY_PILL`), and a coin's flip (`floats::COIN`). The words are the web
//! interface's (`ui/src/stage/notices.ts` and `ui/src/game/TurnBanner.tsx`).
//!
//! - `Prepare` lays out the element of every notice of the playing and waiting
//!   scripts as each update arrives, under the notice's key (`notice::key`), so that a frame
//!   lays nothing out.
//! - `Draw` shows in each float the notice under way there that began last, as
//!   the web's `announced` picks one, and sets its look: it fades in and out over the shares
//!   of its run its kind gives (`notice::Fade`).

use beryl_protocol::protocol::{PlayerView, Side};
use beryl_protocol::script::Notice;
use glam::Vec2;
use std::convert::Infallible;

use crate::art::orb;
use game_play::notice::{Announced, NoticeRun, Place};
use game_table::table::{HudAnchor, Layout};
use kit_base::data::profiles::SizeClass;
use kit_base::tokens::{ratio, size, space};
use kit_scene::{Context, Feature, StateUpdate};
use kit_ui::element::{Element, FloatId, floats};
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
    turn: Option<TurnMetrics>,
}

/// Information from the notice's view needed to name the source and the seats.
pub struct Inputs<'a> {
    pub notice: &'a Notice,
    pub view: &'a PlayerView,
    pub turn_width: f32,
}

impl Feature for NoticeFloats {
    const SOURCE: &'static str = file!();

    type State = Self;
    type Message = Infallible;
    type Inputs<'a> = Inputs<'a>;
    type ViewKey = (Notice, Option<String>, Option<(usize, Option<String>)>, u32);
    type Command = Infallible;
    type Request = Infallible;

    fn update(
        _: &mut Self,
        message: &Infallible,
        _: &mut Context<'_, Infallible, Infallible>,
    ) -> bool {
        match *message {}
    }

    fn view_key(_: &Self, input: &Inputs<'_>) -> Self::ViewKey {
        (
            input.notice.clone(),
            input.coin_source().map(str::to_owned),
            input
                .roll_seats()
                .map(|(seat, peer)| (seat, peer.map(str::to_owned))),
            input.turn_width.to_bits(),
        )
    }

    fn view(_: &Self, input: Inputs<'_>) -> Element {
        // A notice only tells what happened. A press reaches the card beneath it.
        view::element(&input)
            .name(name_of(Place::of(input.notice)))
            .passes(true)
    }
}

impl Inputs<'_> {
    fn coin_source(&self) -> Option<&str> {
        let Notice::Coin { source, .. } = self.notice else {
            return None;
        };
        self.view
            .objects
            .iter()
            .find(|object| object.slot == *source)?
            .name
            .as_deref()
    }

    fn roll_seats(&self) -> Option<(usize, Option<&str>)> {
        let Notice::Roll { outcomes, .. } = self.notice else {
            return None;
        };
        let table = self
            .view
            .table
            .as_ref()
            .filter(|table| table.choosing_first && outcomes.len() == 2)?;
        Some((
            usize::from(table.seat.unwrap_or(0)),
            table.peer_name.as_deref(),
        ))
    }
}

impl super::FloatFeature for NoticeFloats {
    fn floats() -> impl Iterator<Item = FloatId> {
        FLOATS.into_iter().map(|(id, _)| id)
    }
}

/// Lays out the notices of playing and upcoming scripts.
pub struct Prepare<'a, Coming> {
    pub layer: &'a mut Layer,
    pub coming: Coming,
    pub held: &'a [Announced],
    /// Width of the regular window in logical points.
    pub window_width: f32,
    pub size_class: SizeClass,
}

impl<'a, Coming> StateUpdate<NoticeFloats> for Prepare<'_, Coming>
where
    Coming: Iterator<Item = (usize, &'a [NoticeRun], &'a PlayerView)>,
{
    type Output = ();

    fn apply(self, state: &mut NoticeFloats) -> (bool, ()) {
        let turn = TurnMetrics::for_window(self.window_width, self.size_class);
        state.prepare(self.layer, self.coming, self.held, turn);
        (false, ())
    }
}

/// Samples the prepared notices under way in a playback frame.
pub struct Draw<'a> {
    pub layer: &'a mut Layer,
    pub notices: &'a [Announced],
    pub now: u64,
    pub layout: &'a Layout,
    pub visible: bool,
}

impl StateUpdate<NoticeFloats> for Draw<'_> {
    type Output = ();

    fn apply(self, state: &mut NoticeFloats) -> (bool, ()) {
        state.write(
            self.layer,
            self.notices,
            self.now,
            self.layout,
            self.visible,
        );
        (false, ())
    }
}

impl NoticeFloats {
    /// Lays out in `layer` the element of each notice in `coming` (`Show::coming_notices`),
    /// and forgets those laid out before that are neither coming nor in `held`
    /// (`Show::notices`). The client calls it as each update arrives, so that `write` lays
    /// nothing out and allocates nothing.
    fn prepare<'a>(
        &mut self,
        layer: &mut Layer,
        coming: impl Iterator<Item = (usize, &'a [NoticeRun], &'a PlayerView)>,
        held: &[Announced],
        turn: TurnMetrics,
    ) {
        if self.turn != Some(turn) {
            if self.shown[0].is_some_and(is_turn_key) {
                layer.show_prepared(floats::BANNER, None);
                self.shown[0] = None;
            }
            layer.forget_prepared(floats::BANNER, |key| !is_turn_key(key));
            self.turn = Some(turn);
        }
        for active in held.iter().filter_map(|notice| notice.turn) {
            Self::prepare_turn(layer, active, turn.width);
        }
        let mut keys: Vec<u64> = held.iter().map(|announced| announced.key).collect();
        for (script, runs, view) in coming {
            for run in runs {
                if let Notice::Turn { active } = run.notice {
                    Self::prepare_turn(layer, active, turn.width);
                    continue;
                }
                let key = game_play::notice::key(script, run.step);
                keys.push(key);
                let place = Place::of(&run.notice);
                layer.prepare(float_of(place), key, || {
                    Self::view(
                        self,
                        Inputs {
                            notice: &run.notice,
                            view,
                            turn_width: turn.width,
                        },
                    )
                });
            }
        }
        for (id, _) in FLOATS {
            layer.forget_prepared(id, |key| is_turn_key(key) || keys.contains(&key));
        }
    }

    fn prepare_turn(layer: &mut Layer, side: Side, width: f32) {
        layer.prepare(floats::BANNER, turn_key(side), || {
            view::turn_banner(side, width).name("banner").passes(true)
        });
    }

    /// Shows in `layer` the notices of `notices` under way at `now`, placed for `layout`, or none of them unless `visible`. Lays nothing out and allocates
    /// nothing for what `prepare` laid out.
    fn write(
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
            let want = showing.map(|announced| announced.turn.map_or(announced.key, turn_key));
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
            let units = if announced.turn.is_some() {
                self.turn.map_or(1.0, |turn| turn.scale)
            } else {
                1.0
            };
            let slide = Vec2::Y * slide(announced, now) * units;
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
                // The orb is drawn over floats, so a pill must clear the orb's art, not just
                // the number: it sits on the side of the orb that faces the middle of the
                // table, above your orb and below the opponent's.
                (Place::Pill(player), false) => {
                    let clear = orb::ART[3] * layout.play.life.orb_scale / 2.0 + space::S;
                    match player {
                        Side::You => HudAnchor::Life(player).at(
                            Vec2::ZERO,
                            Vec2::new(0.5, 1.0),
                            slide - Vec2::Y * clear,
                        ),
                        Side::Opponent => HudAnchor::Life(player).at(
                            Vec2::ZERO,
                            Vec2::new(0.5, 0.0),
                            slide + Vec2::Y * clear,
                        ),
                    }
                }
            };
            layer.anchor_float(
                id,
                anchor,
                FloatLook {
                    opacity: opacity(announced, now),
                    scale: scale(announced, now) * units,
                    icon_progress: progress(announced, now),
                    ..FloatLook::default()
                },
            );
        }
    }
}

/// The turn banner fills the stage on desktop and uses the compact banner's fixed width and scale
/// on a phone.
#[derive(Clone, Copy, Debug, PartialEq)]
struct TurnMetrics {
    width: f32,
    scale: f32,
}

impl TurnMetrics {
    fn for_window(width: f32, size_class: SizeClass) -> Self {
        match size_class {
            SizeClass::Regular => Self { width, scale: 1.0 },
            SizeClass::Compact => Self {
                width: size::TURN_BANNER_COMPACT_WIDTH,
                scale: ratio::TURN_BANNER_COMPACT_SCALE,
            },
        }
    }
}

// Each side reuses one prepared turn banner until the window changes. Script keys occupy the
// low end of the range (`notice::key`).
const fn turn_key(side: Side) -> u64 {
    match side {
        Side::You => u64::MAX,
        Side::Opponent => u64::MAX - 1,
    }
}

const fn is_turn_key(key: u64) -> bool {
    key >= u64::MAX - 1
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
