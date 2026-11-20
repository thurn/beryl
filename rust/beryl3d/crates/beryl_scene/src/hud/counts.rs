//! The numbers a `count` step changes that are not on a card: each player's life total and
//! unspent mana (docs/beryl-3d.md, "Effects" and "Floats").
//!
//! - The verb writes each `Tally::Life` and `Tally::Pool` as a `CountChange` at the start of its
//!   step, and the show applies the change to its `Counts` when the script's clock reaches it.
//!   As a script ends, `Counts` takes the numbers of the view it leads to. So a total changes as
//!   its step begins, not when the view that holds it arrives.
//! - `CountFloats` shows `Counts` as floats of the flat layer: each life total
//!   (`floats::LIFE`), each pool while it holds mana (`floats::POOL`), and while a `loss` or
//!   `gain` effect plays on a player, the amount their total changed by (`floats::LIFE_CHANGE`).
//!   Every text a float will show is laid out ahead, as the update whose script sets it
//!   arrives (`CountFloats::prepare`), so that a frame only chooses which laid-out element each
//!   float shows and sets its look, and allocates nothing.

use std::fmt::Write as _;

use beryl_protocol::prompt::Click;
use beryl_protocol::protocol::{ManaPool, PlayerView, Side};
use beryl_protocol::script::Subject;
use glam::Vec2;

use crate::art::orb;
use crate::data::space::verbs::count::DRIFT;
use crate::effects::{Effect, EffectRun};
use crate::element::{Align, Element, FloatId, Press, Span, TextAlign, floats, holder, text};
use crate::frame::Role;
use crate::input::Size;
use crate::layout::{FloatLook, Layer};
use crate::table::Layout;
use crate::tokens::{color, size, type_style};

/// A number a `count` step sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Count {
    Life(i32),
    Pool(ManaPool),
}

/// One player's number set at `at`: on the script's clock as a verb writes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CountChange {
    pub player: Side,
    pub count: Count,
    pub at: u64,
    /// The number of the step that wrote it.
    pub step: usize,
}

/// One player's numbers as the table shows them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlayerCounts {
    pub life: i32,
    pub pool: ManaPool,
    /// How much the last change to `life` changed it by, which a `loss` or `gain` shows.
    pub change: i32,
}

/// Both players' numbers as the table shows them, the viewer's first; none before the first
/// script has ended.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    players: Option<[PlayerCounts; 2]>,
}

impl Counts {
    /// `side`'s numbers, once the table shows a view.
    #[must_use]
    pub fn player(&self, side: Side) -> Option<&PlayerCounts> {
        Some(&self.players.as_ref()?[index(side)])
    }

    /// Takes the life totals and pools of `view`, which the table now shows at rest. The last
    /// change is kept, because the `loss` or `gain` that shows it may outlast its script.
    pub(crate) fn take_view(&mut self, view: &PlayerView) {
        let players = self.players.get_or_insert_default();
        for (counts, state) in players
            .iter_mut()
            .zip([&view.players.you, &view.players.opponent])
        {
            counts.life = state.life;
            counts.pool = state.mana;
        }
    }

    /// Applies one change a `count` step wrote.
    pub(crate) fn apply(&mut self, change: &CountChange) {
        let Some(players) = &mut self.players else {
            return;
        };
        let counts = &mut players[index(change.player)];
        match change.count {
            Count::Life(life) => {
                counts.change = life - counts.life;
                counts.life = life;
            }
            Count::Pool(mana) => counts.pool = mana,
        }
    }
}

/// The index of `side` in pairs that put the viewer first.
pub(crate) const fn index(side: Side) -> usize {
    match side {
        Side::You => 0,
        Side::Opponent => 1,
    }
}

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
    /// Whether the window has the phone layout, where a total is drawn smaller, on a smaller orb.
    pub phone: bool,
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
        targets: [bool; 2],
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
                        .map(|t| total_key(life, t, self.passes, self.phone))
                        .contains(&key)
                })
            });
            for &life in &lives[at] {
                for target in [false, true] {
                    if target && !targets[at] {
                        continue;
                    }
                    let (passes, phone) = (self.passes, self.phone);
                    layer.prepare(id, total_key(life, target, passes, phone), || {
                        life_total((side, name), Some(life), (target, passes, phone))
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

    /// Shows `counts` in `layer` as they stand at `now`, where `layout` puts each player's total
    /// and pool in a window of `size`, or shows none of them unless `visible`. A `loss` or `gain`
    /// in `effects` that plays on a player at `now` tints their total and shows by how much it
    /// changed, sinking away for a loss and rising for a gain while it fades, linearly over the
    /// run. A press on the total of a player marked in `targets` picks them. While `dimmed`, the
    /// opening's backdrop lies under the totals, which are dimmed as it dims what lies under it.
    /// Lays nothing out and allocates nothing for what `prepare` laid out.
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
        layout: &Layout,
        size: Size,
        (visible, targets, dimmed): (bool, [bool; 2], bool),
    ) {
        let (passes, phone) = (self.passes, self.phone);
        for (side, name) in [(Side::You, "You"), (Side::Opponent, "Opponent")] {
            let at = index(side);
            let target = targets[at];
            // The phone's opening hides the orbs, and the totals with them
            // (`screens::game_phone`). The desktop's coin and the words saying who starts stand
            // over the opponent's total, as the web's coin covers it.
            let hidden = dimmed && (phone || side == Side::Opponent);
            let counts = counts.player(side).filter(|_| visible && !hidden);
            let run = effects.iter().find(|run| {
                matches!(run.effect, Effect::Loss | Effect::Gain)
                    && run.on == Subject::Player { player: side }
                    && run.start <= now
                    && now < run.start + run.duration
            });
            let want = Shown {
                life: counts.map(|counts| (counts.life, target, passes)),
                pool: counts
                    .filter(|_| self.pools)
                    .map(|counts| counts.pool)
                    .filter(|pool| *pool != ManaPool::default()),
                change: counts.zip(run).map(|(counts, _)| counts.change),
            };
            let shown = &mut self.shown[at];
            if want.life != shown.life {
                let id = floats::LIFE[at];
                show(
                    layer,
                    id,
                    want.life.map(|(life, t, p)| total_key(life, t, p, phone)),
                    || {
                        want.life.and_then(|(life, t, p)| {
                            life_total((side, name), Some(life), (t, p, phone))
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
            let units = orb::SCALES[usize::from(phone)];
            let life = points(anchor.life, size) + Vec2::new(0.0, orb::face_offset(side) * units);
            let tint = run.map(|run| match run.effect {
                Effect::Gain => color::GAIN,
                _ => color::LOSS,
            });
            // Under the opening's backdrop the total is dimmed as the orb under it is.
            let opacity = if dimmed {
                1.0 - crate::screens::opening::backdrop_cover(life.y, size.height)
            } else {
                1.0
            };
            layer.set_float_look(
                floats::LIFE[at],
                FloatLook {
                    point: life,
                    anchor: Vec2::new(0.5, 0.5),
                    opacity,
                    tint,
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
            if let Some(run) = run {
                layer.set_float_look(
                    floats::LIFE_CHANGE[at],
                    change_look(run, now, life, tint, phone),
                );
            }
        }
    }
}

/// How far, in points, a gain's number on the desktop layout starts above where the web's
/// `.life-change-gain` puts it. At the web's position the number's box reaches 3 points into the
/// "Your draw" step toggle at the orb's top left, which "text overlaps nothing" rejects now that
/// the steps stay up while scripts play, and the number is a transient float, so 2 points do not
/// show.
const GAIN_CLEARANCE: f32 = 2.0;

/// How far a line of the title type reaches above and below its middle, in logical points: the
/// phone's life total, from which the amount it changed by starts, and the space the desktop's
/// notices leave above a total.
pub(crate) const HALF_LIFE: f32 = type_style::TITLE.size * type_style::TITLE.line_height / 2.0;

/// The look of the amount a life total at `life` changed by, as the `loss` or `gain` `run`
/// stands at `now`: from beside the total, it sinks below it for a loss, or rises above it for
/// a gain, `DRIFT` points over the run, while its opacity falls from 1 to 0.
fn change_look(
    run: &EffectRun,
    now: u64,
    life: Vec2,
    tint: Option<[u8; 4]>,
    phone: bool,
) -> FloatLook {
    #[expect(
        clippy::cast_precision_loss,
        reason = "an effect lasts seconds at most"
    )]
    let through = if run.duration == 0 {
        1.0
    } else {
        (now - run.start) as f32 / run.duration as f32
    };
    // The web positions the loss from the orb art's bottom (14 points in), and the
    // gain from its top (4 points in), rather than from the total's line box.
    let half = orb::ART[3] * orb::SCALES[usize::from(phone)] / 2.0;
    let height = type_style::TITLE.size * type_style::TITLE.line_height;
    let drift = DRIFT * through;
    let (offset, anchor) = if phone && run.effect == Effect::Gain {
        (-HALF_LIFE - drift, Vec2::new(0.5, 1.0))
    } else if phone {
        (HALF_LIFE + drift, Vec2::new(0.5, 0.0))
    } else if run.effect == Effect::Gain {
        (
            -half + size::LIFE_CHANGE_TOP - GAIN_CLEARANCE - drift,
            Vec2::new(1.0, 0.0),
        )
    } else {
        (
            half - size::LIFE_CHANGE_BOTTOM - height + drift,
            Vec2::new(1.0, 0.0),
        )
    };
    // The web's `.life-change` ends left of the orb's centre, so a loss beside the
    // opponent's total cannot drift through the Combat banner below it.
    let left = if phone { 0.0 } else { size::LIFE_CHANGE_LEFT };
    FloatLook {
        point: life + Vec2::new(-left, offset),
        anchor,
        opacity: 1.0 - through,
        tint,
        scale: 1.0,
    }
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

/// The key a life total is prepared under, as a target of a prompt's or not, and on the phone
/// layout or not.
fn total_key(life: i32, target: bool, passes: bool, phone: bool) -> u64 {
    life_key(life) | u64::from(target) << 32 | u64::from(passes) << 33 | u64::from(phone) << 34
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

/// A player's life total, if any: a player element that shows the number, named for whose it
/// is and the number, as "You 20", so that the tree's line for it says both. As a `target`, a
/// press on it clicks the open prompt on the player, as the web interface's `PlayerTarget`
/// (`ui/src/stage/PromptBar.tsx`) does.
///
/// On the desktop it is the web's `.life-orb-total`, large and bold in the cream of
/// `color::LIFE`; the phone's smaller orb keeps the prompt's title size. A float's top element
/// is drawn on a panel unless it says otherwise, and the total stands on its orb with none.
fn life_total(
    (side, name): (Side, &str),
    life: Option<i32>,
    (target, passes, phone): (bool, bool, bool),
) -> Option<Element> {
    let life = life?;
    let mut element = Element::new(Role::Player)
        .name(&format!("{name} {life}"))
        .style(if phone {
            type_style::TITLE
        } else {
            type_style::LIFE
        })
        .color(color::LIFE)
        .background([0; 4])
        .border(([0; 4], 0.0));
    // A total the prompt can pick is a touch target as large as an icon button, the phone's
    // least, centred on the orb as the total is, so the orb around it still takes no press.
    // Any other total on the game screen lets the pointer through, as the orb does, to a card
    // of the phone's hand that stands behind it.
    if target {
        element = element
            .on_press(Press::Prompt(Click::On {
                on: Subject::Player { player: side },
            }))
            .min_width(size::ICON_BUTTON)
            .min_height(size::ICON_BUTTON)
            .text_align(TextAlign::Center);
    } else {
        element = element.passes(passes);
    }
    element.text = Some(life.to_string());
    Some(element)
}

/// A pool of unspent mana, as the web interface's `ManaPool` (`ui/src/components/Player.tsx`)
/// shows it: large mana symbols 6 points apart, each followed by its count in small bold type
/// when there is more than one (`.mana-pool` in
/// `ui/src/design/components/mana-pool-and-coin.css`), in the order W, U, B, R, G and then
/// colorless. The flat layer draws each `{U}` as an inline mana symbol, a disc the size of the
/// type's em.
fn pool_text(pool: ManaPool) -> Element {
    holder()
        .name(&mana(pool))
        .gap(size::POOL_GAP)
        .align_items(Align::Center)
        .passes(true)
        .children(entries(pool).map(|(count, symbol)| {
            let mut words = format!("{{{symbol}}}");
            let symbol_end = words.len();
            if count > 1 {
                let _ = write!(words, "{count}");
            }
            let mut entry = text(&words).style(type_style::POOL);
            #[expect(clippy::cast_possible_truncation, reason = "a short text")]
            let range = symbol_end as u32..words.len() as u32;
            if !range.is_empty() {
                entry.spans.push(Span {
                    range,
                    style: type_style::POOL_COUNT,
                    color: None,
                });
            }
            entry
        }))
}

/// Each kind of mana in `pool`, with how much of it there is, in the order W, U, B, R, G and
/// then colorless, leaving out the kinds there is none of.
fn entries(pool: ManaPool) -> impl Iterator<Item = (u32, char)> {
    [
        (pool.white, 'W'),
        (pool.blue, 'U'),
        (pool.black, 'B'),
        (pool.red, 'R'),
        (pool.green, 'G'),
        (pool.colorless, 'C'),
    ]
    .into_iter()
    .filter(|&(count, _)| count > 0)
}

/// By how much a life total changed, with its sign.
fn life_change(by: i32) -> Element {
    text(&format!("{by:+}"))
        .style(type_style::TITLE)
        .passes(true)
}

/// Unspent mana as text, the pool's name in the tree: one mana symbol for each kind there is,
/// followed by its count when that is more than one, as the web's `ManaPool` shows it, as
/// "{U}2 {G}".
fn mana(pool: ManaPool) -> String {
    let mut out = String::new();
    for (count, symbol) in entries(pool) {
        if !out.is_empty() {
            out.push(' ');
        }
        let _ = write!(out, "{{{symbol}}}");
        if count > 1 {
            let _ = write!(out, "{count}");
        }
    }
    out
}

/// A point in normalized device coordinates as logical points of a window of `size`.
pub(crate) fn points(ndc: Vec2, size: Size) -> Vec2 {
    Vec2::new(
        f32::midpoint(ndc.x, 1.0) * size.width,
        f32::midpoint(1.0, -ndc.y) * size.height,
    )
}
