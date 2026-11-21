//! Prepared life totals, mana pools and changes to life.

use beryl_protocol::protocol::{ManaPool, PlayerView, Side};
use beryl_protocol::script::Subject;
use glam::Vec2;

use game_table::table::HALF_LIFE;
use game_table::table::Layout;
use kit_base::data::profiles::{Life, LifeChange, LifeText};
use kit_base::data::{Data, effect::NumberOrigin};
use kit_base::tokens::{color, size, type_style};
use kit_fx::effects::{self, EffectRun, NumberLook};
use kit_ui::element::{Element, FloatId, floats};
use kit_ui::input::{Size, Tier};
use kit_ui::layout::{FloatLook, Layer};

pub use game_play::hud::counts::{Count, CountChange, Counts, PlayerCounts, index, points};

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
    pub passes: bool,

    /// Whether each player's unspent mana is shown: not on the manual table, whose own row of
    /// mana and counters stands there (`manual::pools`).
    pub pools: bool,
}

impl CountFloats {
    /// Lays out ahead, in `layer`, each life total, change and pool that `counts` and the
    /// scripts to come (`Show::coming_counts`) will show, and forgets those laid out before that
    /// none of them will. The client calls it as each update arrives, so that `write` lays
    /// nothing out and allocates nothing.
    ///
    /// A player marked in `targets` (the viewer's first) has each total laid out a second time,
    /// as the target a press on picks them by.
    pub fn prepare<'a>(
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
        for (side, name) in [(Side::You, "You"), (Side::Opponent, "Opponent")] {
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
                        life_total((side, name), Some(life), (target, passes, style))
                            .expect("a total")
                    });
                }
            }
            let id = floats::LIFE_CHANGE[at];
            layer.forget_prepared(id, |key| changes[at].iter().any(|&by| life_key(by) == key));
            for &by in &changes[at] {
                layer.prepare(id, life_key(by), || life_change(by));
            }
            let id = floats::POOL[at];
            pools[at].retain(|pool| *pool != ManaPool::default());
            layer.forget_prepared(id, |key| {
                pools[at].iter().any(|&pool| pool_key(pool) == key)
            });
            for &pool in &pools[at] {
                layer.prepare(id, pool_key(pool), || pool_text(pool));
            }
        }
    }

    /// Shows the prepared totals and pools where the layout puts them. A text effect may
    /// tint a total or float its prepared change. Targetable totals respond to presses.
    /// Under the opening's backdrop, the totals dim with their orbs.
    /// The first visible text effect on each total supplies its tint and number.
    /// [`Self::prepare`] lays out the numbers in advance, including those whose
    /// effects begin after the count step.
    #[expect(
        clippy::too_many_arguments,
        reason = "each is a separate input of the frame"
    )]
    pub fn write(
        &mut self,
        layer: &mut Layer,
        counts: &Counts,
        effects: &[EffectRun],
        now: u64,
        (layout, data, tier): (&Layout, &Data, Tier),
        size: Size,
        (visible, targets, dimmed): (bool, [bool; 2], bool),
    ) {
        let passes = self.passes;
        let life_style = layout.play.life;
        let style = life_style.text;
        for (side, name) in [(Side::You, "You"), (Side::Opponent, "Opponent")] {
            let at = index(side);
            let target = targets[at];
            let hidden = dimmed && (life_style.opening_hides_viewer || side == Side::Opponent);
            let counts = counts.player(side).filter(|_| visible && !hidden);
            let effect = effects
                .iter()
                .filter(|run| run.on == Subject::Player { player: side })
                .find_map(|&run| effects::text(&data.effects, run, now, tier, color::LIFE))
                .unwrap_or_default();
            let want = Shown {
                life: counts.map(|counts| (counts.life, target, passes)),
                pool: counts
                    .filter(|_| self.pools)
                    .map(|counts| counts.pool)
                    .filter(|pool| *pool != ManaPool::default()),
                change: counts.zip(effect.number).map(|(counts, _)| counts.change),
            };
            let shown = &mut self.shown[at];
            if want.life != shown.life {
                let id = floats::LIFE[at];
                show(
                    layer,
                    id,
                    want.life.map(|(life, t, p)| total_key(life, t, p, style)),
                    || {
                        want.life.and_then(|(life, t, p)| {
                            life_total((side, name), Some(life), (t, p, style))
                        })
                    },
                );
            }
            if want.pool != shown.pool {
                let id = floats::POOL[at];
                show(layer, id, want.pool.map(pool_key), || {
                    want.pool.map(pool_text)
                });
            }
            if want.change != shown.change {
                let id = floats::LIFE_CHANGE[at];
                show(layer, id, want.change.map(life_key), || {
                    want.change.map(life_change)
                });
            }
            *shown = want;
            let anchor = &layout.players[at];
            // The web centres the total on the orb's core, a few points off its art's middle.
            let units = life_style.orb_scale;
            let life = points(anchor.life, size) + Vec2::new(0.0, orb::face_offset(side) * units);
            // Under the opening's backdrop the total is dimmed as the orb under it is.
            let opacity = if dimmed {
                1.0 - backdrop::backdrop_cover(life.y, size.height)
            } else {
                1.0
            };
            layer.set_float_look(
                floats::LIFE[at],
                FloatLook {
                    point: life,
                    anchor: Vec2::new(0.5, 0.5),
                    opacity,
                    tint: effect.tint,
                    scale: 1.0,
                },
            );
            layer.set_float_look(
                floats::POOL[at],
                FloatLook {
                    point: points(anchor.pool, size),
                    ..FloatLook::default()
                },
            );
            if let Some(number) = effect.number {
                layer.set_float_look(
                    floats::LIFE_CHANGE[at],
                    change_look(
                        number,
                        life,
                        effect.tint,
                        life_style,
                        data.verbs.count.drift,
                    ),
                );
            }
        }
    }
}

/// Keep the desktop gain above the position where it overlaps the "Your draw" toggle.
const GAIN_CLEARANCE: f32 = 2.0;

/// Places the sampled number beside the total at `life`, using the orb's top or bottom.
fn change_look(
    number: NumberLook,
    life: Vec2,
    tint: Option<[u8; 4]>,
    style: Life,
    drift: f32,
) -> FloatLook {
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
    number.place(life + Vec2::new(-left, offset), anchor, tint)
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
