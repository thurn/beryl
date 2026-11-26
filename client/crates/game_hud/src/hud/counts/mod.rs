//! Prepared life totals, mana pools and changes to life.

use beryl_protocol::prompt::Click;
use beryl_protocol::protocol::{ManaPool, PlayerView, Side};
use beryl_protocol::script::Subject;
use glam::Vec2;

use game_table::table::{HALF_LIFE, HudAnchor, Table};
use kit_base::data::effect::NumberOrigin;
use kit_base::data::profiles::{Life, LifeChange, LifeText};
use kit_base::tokens::{color, ratio, size, type_style};
use kit_fx::effects::{self, EffectRun, NumberLook};
use kit_scene::{Context, Feature, StateUpdate};
use kit_ui::element::{Element, FloatId, floats};
use kit_ui::input::Tier;
use kit_ui::layout::{FloatLook, Layer};
use std::convert::Infallible;

pub use game_play::hud::counts::{Count, CountChange, Counts, PlayerCounts, index};

use crate::art::orb;

mod backdrop;
pub mod players;
mod view;
use view::{life_change, life_total, pool_text};

/// What one player's floats show now, so that a float is shown again only when it changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Shown {
    /// The total, whether a press on it picks the player, and whether it lets the pointer
    /// through (`CountFloats::passes`).
    life: Option<(i32, bool, bool)>,
    pool: Option<ManaPool>,
    change: Option<i32>,
}

/// The floats that show `Counts`, and what each shows now.
#[derive(Debug, Default)]
pub struct CountFloats {
    shown: [Shown; 2],
    /// Whether a life total no prompt can pick lets the pointer through to what lies under it:
    /// on the game screen, where the phone's hand stands behind it, but not on the manual
    /// table, whose commands act on the total under the pointer.
    passes: bool,

    /// Whether each player's unspent mana is shown: not on the manual table, whose own row of
    /// mana and counters stands there (`manual::pools`).
    pools: bool,
}

/// Selects a player as the target of the open prompt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Message {
    Target(Side),
}

/// The data needed to build one life total, life change or mana pool.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Inputs {
    Total(Side, i32, bool, bool, LifeText),
    Change(i32),
    Pool(ManaPool),
}

impl Feature for CountFloats {
    const SOURCE: &'static str = file!();

    type State = Self;
    type Message = Message;
    type Inputs<'a> = Inputs;
    type ViewKey = Inputs;
    type Command = Click;
    type Request = Infallible;

    fn update(
        _: &mut Self,
        message: &Message,
        context: &mut Context<'_, Click, Infallible>,
    ) -> bool {
        let Message::Target(player) = *message;
        context.command(Click::On {
            on: Subject::Player { player },
        });
        false
    }

    fn view_key(_: &Self, input: &Inputs) -> Inputs {
        *input
    }

    fn view(_: &Self, input: Inputs) -> Element {
        match input {
            Inputs::Total(side, life, target, passes, style) => {
                life_total(side, life, (target, passes, style))
            }
            Inputs::Change(by) => life_change(by),
            Inputs::Pool(pool) => pool_text(pool),
        }
    }
}

impl super::FloatFeature for CountFloats {
    fn floats() -> impl Iterator<Item = FloatId> {
        floats::LIFE
            .into_iter()
            .chain(floats::LIFE_CHANGE)
            .chain(floats::POOL)
    }
}

/// Prepares all totals and pools that playback can show.
pub struct Prepare<'a, Coming> {
    pub layer: &'a mut Layer,
    pub counts: &'a Counts,
    pub coming: Coming,
    pub targets: [bool; 2],
    pub style: LifeText,
    pub passes: bool,
    pub pools: bool,
}

impl<'a, Coming> StateUpdate<CountFloats> for Prepare<'_, Coming>
where
    Coming: Iterator<Item = (&'a [CountChange], &'a PlayerView)>,
{
    type Output = ();

    fn apply(self, state: &mut CountFloats) -> (bool, ()) {
        state.passes = self.passes;
        state.pools = self.pools;
        state.prepare(
            self.layer,
            self.counts,
            self.coming,
            (self.targets, self.style),
        );
        (false, ())
    }
}

/// Samples prepared totals and pools for one playback frame.
pub struct Draw<'a> {
    pub layer: &'a mut Layer,
    pub counts: &'a Counts,
    pub effects: &'a [EffectRun],
    pub now: u64,
    pub table: &'a Table,
    pub tier: Tier,
    pub visible: bool,
    pub targets: [bool; 2],
    pub dimmed: bool,
}

impl StateUpdate<CountFloats> for Draw<'_> {
    type Output = ();

    fn apply(self, state: &mut CountFloats) -> (bool, ()) {
        state.write(
            self.layer,
            self.counts,
            self.effects,
            self.now,
            (self.table, self.tier),
            (self.visible, self.targets, self.dimmed),
        );
        (false, ())
    }
}

impl CountFloats {
    /// Lays out ahead, in `layer`, each life total, change and pool that `counts` and the
    /// scripts to come (`Show::coming_counts`) will show, and forgets those laid out before that
    /// none of them will. The client calls it as each update arrives, so that `write` lays
    /// nothing out and allocates nothing.
    ///
    /// A player marked in `targets` (the viewer's first) has each total laid out a second time,
    /// as the target a press on picks them by.
    fn prepare<'a>(
        &mut self,
        layer: &mut Layer,
        counts: &Counts,
        coming: impl Iterator<Item = (&'a [CountChange], &'a PlayerView)>,
        (targets, style): ([bool; 2], LifeText),
    ) {
        // What each player's floats will show, as the scripts play out.
        let mut lives: [Vec<i32>; 2] = Default::default();
        let mut changes: [Vec<i32>; 2] = Default::default();
        let mut pools: [Vec<ManaPool>; 2] = Default::default();
        let mut note = |state: &Counts| {
            for side in [Side::You, Side::Opponent] {
                if let Some(counts) = state.player(side) {
                    let at = index(side);
                    lives[at].push(counts.life);
                    changes[at].push(counts.change);
                    pools[at].push(counts.pool);
                }
            }
        };
        let mut state = *counts;
        note(&state);
        for (changes, view) in coming {
            for change in changes {
                state.apply(change);
                note(&state);
            }
            state.take_view(view);
            note(&state);
        }
        for side in [Side::You, Side::Opponent] {
            let at = index(side);
            let id = floats::LIFE[at];
            layer.forget_prepared(id, |key| {
                (lives[at].iter()).any(|&life| {
                    [false, true]
                        .map(|t| total_key(life, t, self.passes, style))
                        .contains(&key)
                })
            });
            for &life in &lives[at] {
                for target in [false, true] {
                    if target && !targets[at] {
                        continue;
                    }
                    let passes = self.passes;
                    layer.prepare(id, total_key(life, target, passes, style), || {
                        Self::view(self, Inputs::Total(side, life, target, passes, style))
                    });
                }
            }
            let id = floats::LIFE_CHANGE[at];
            layer.forget_prepared(id, |key| changes[at].iter().any(|&by| life_key(by) == key));
            for &by in &changes[at] {
                layer.prepare(id, life_key(by), || Self::view(self, Inputs::Change(by)));
            }
            let id = floats::POOL[at];
            pools[at].retain(|pool| *pool != ManaPool::default());
            layer.forget_prepared(id, |key| {
                pools[at].iter().any(|&pool| pool_key(pool) == key)
            });
            for &pool in &pools[at] {
                layer.prepare(id, pool_key(pool), || Self::view(self, Inputs::Pool(pool)));
            }
        }
    }

    /// Shows the prepared totals and pools where the layout puts them. A text effect may
    /// tint a total or float its prepared change. Targetable totals respond to presses.
    /// Under the opening's backdrop, the totals dim with their orbs.
    /// The first visible text effect on each total supplies its tint and number.
    /// [`Self::prepare`] lays out the numbers in advance, including those whose
    /// effects begin after the count step.
    fn write(
        &mut self,
        layer: &mut Layer,
        counts: &Counts,
        effects: &[EffectRun],
        now: u64,
        (table, tier): (&Table, Tier),
        (visible, targets, dimmed): (bool, [bool; 2], bool),
    ) {
        let passes = self.passes;
        let data = table.data();
        let life_style = table.layout().play.life;
        let style = life_style.text;
        for side in [Side::You, Side::Opponent] {
            let at = index(side);
            let target = targets[at];
            let hidden = dimmed && (life_style.opening_hides_viewer || side == Side::Opponent);
            let counts = counts.player(side).filter(|_| visible && !hidden);
            let effect = effects
                .iter()
                .filter(|run| {
                    run.on == game_table::effect::target(Subject::Player { player: side })
                })
                .find_map(|run| effects::text(run, now, tier, color::LIFE))
                .unwrap_or_default();
            let want = Shown {
                life: counts.map(|counts| (counts.life, target, passes)),
                pool: counts
                    .filter(|_| self.pools)
                    .map(|counts| counts.pool)
                    .filter(|pool| *pool != ManaPool::default()),
                change: counts.zip(effect.number).map(|(counts, _)| counts.change),
            };
            let shown = self.shown[at];
            if want.life != shown.life {
                let id = floats::LIFE[at];
                show(
                    layer,
                    id,
                    want.life.map(|(life, t, p)| total_key(life, t, p, style)),
                    || {
                        want.life.map(|(life, target, passes)| {
                            Self::view(self, Inputs::Total(side, life, target, passes, style))
                        })
                    },
                );
            }
            if want.pool != shown.pool {
                let id = floats::POOL[at];
                show(layer, id, want.pool.map(pool_key), || {
                    want.pool.map(|pool| Self::view(self, Inputs::Pool(pool)))
                });
            }
            if want.change != shown.change {
                let id = floats::LIFE_CHANGE[at];
                show(layer, id, want.change.map(life_key), || {
                    want.change.map(|by| Self::view(self, Inputs::Change(by)))
                });
            }
            self.shown[at] = want;
            let art_offset = orb::face_offset(side) * life_style.orb_scale;
            let offset = Vec2::new(0.0, art_offset);
            let life = HudAnchor::Life(side);
            // The float draws above the backdrop, so apply both the orb's opening opacity
            // and the backdrop's dimming to the number.
            let opacity = if dimmed {
                let cover = backdrop::backdrop_cover(
                    table.life_centre(side).y + art_offset,
                    table.camera().window.size.height,
                );
                ratio::OPENING_HUD_OPACITY * (1.0 - cover)
            } else {
                1.0
            };
            layer.anchor_float(
                floats::LIFE[at],
                life.at(Vec2::ZERO, Vec2::splat(0.5), offset),
                FloatLook {
                    opacity,
                    tint: effect.tint,
                    ..FloatLook::default()
                },
            );
            layer.anchor_float(
                floats::POOL[at],
                HudAnchor::Pool(side).at(Vec2::ZERO, table.pool_align(), Vec2::ZERO),
                FloatLook::default(),
            );
            if let Some(number) = effect.number {
                let (align, drift) = change_placement(number, life_style, data.verbs.count.drift);
                layer.anchor_float(
                    floats::LIFE_CHANGE[at],
                    life.at(Vec2::ZERO, align, offset + drift),
                    FloatLook {
                        opacity: number.opacity,
                        tint: effect.tint,
                        ..FloatLook::default()
                    },
                );
            }
        }
    }
}

/// Keep the desktop gain above the position where it overlaps the "Your draw" toggle.
const GAIN_CLEARANCE: f32 = 2.0;

/// The sampled number's alignment and offset relative to the life total.
fn change_placement(number: NumberLook, style: Life, drift: f32) -> (Vec2, Vec2) {
    let drift = drift * number.drift;
    let above = number.origin == NumberOrigin::Above;
    let (left, offset, anchor) = match style.change {
        LifeChange::AroundTotal if above => (0.0, -HALF_LIFE + drift, Vec2::new(0.5, 1.0)),
        LifeChange::AroundTotal => (0.0, HALF_LIFE + drift, Vec2::new(0.5, 0.0)),
        LifeChange::BesideOrb => {
            // Measure the gain from the top of the orb art and the loss from its bottom.
            let half = orb::ART[3] * style.orb_scale / 2.0;
            let height = type_style::TITLE.size * type_style::TITLE.line_height;
            let offset = if above {
                -half + size::LIFE_CHANGE_TOP - GAIN_CLEARANCE + drift
            } else {
                half - size::LIFE_CHANGE_BOTTOM - height + drift
            };
            (size::LIFE_CHANGE_LEFT, offset, Vec2::new(1.0, 0.0))
        }
    };
    (anchor, Vec2::new(-left, offset))
}

/// Shows as the float `id` the element prepared under `key`, or nothing. Where none was
/// prepared, which `CountFloats::prepare` should prevent, it is made by `element` and laid out
/// now.
fn show(
    layer: &mut Layer,
    id: FloatId,
    key: Option<u64>,
    element: impl FnOnce() -> Option<Element>,
) {
    if !layer.show_prepared(id, key) {
        debug_assert!(false, "nothing is prepared under {key:?} for float {id:?}");
        layer.float(id, element());
    }
}

/// The key for a life total, including its interaction and text style.
fn total_key(life: i32, target: bool, passes: bool, style: LifeText) -> u64 {
    life_key(life)
        | u64::from(target) << 32
        | u64::from(passes) << 33
        | u64::from(style == LifeText::Title) << 34
}

/// The key a life total or a change of one is prepared under.
fn life_key(life: i32) -> u64 {
    u64::from(life.cast_unsigned())
}

/// The key a pool is prepared under: ten bits for each kind of mana, which tells apart every
/// pool of fewer than 1,024 of each.
fn pool_key(pool: ManaPool) -> u64 {
    [
        pool.white,
        pool.blue,
        pool.black,
        pool.red,
        pool.green,
        pool.colorless,
    ]
    .into_iter()
    .fold(0, |key, count| key << 10 | u64::from(count.min(1023)))
}
