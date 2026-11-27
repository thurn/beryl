//! Draws the streaks of mana and damage that `Lit` holds, as the web stage's `cometOf`
//! (`ui/src/stage/streaks.ts`) shapes them: a bright head on a low arc from its source to its
//! target, with a tail that stretches out of the source, follows, and is drawn in as the head
//! lands. A streak is drawn while its step plays and at no other time.
//!
//! Both ends are read from the scene on each frame, so that a streak to a card in flight lands
//! on it where it is. A streak from a player leaves their pool's anchor. A streak whose source
//! or target has no place on the table is not drawn.
//!
//! The values are `verbs.streak` in `data/client/verbs.json`, the web's `COMET` and
//! `STREAK.ribbon` with its stage pixels in millimetres: the arc rises `bow` of the way between
//! its ends and no more than `rise` of the taller end's height; each pair of streaks after the
//! first two that leave one place for one target together rises `fan` as high again; the tail
//! is `tail` of the way long; the head is `head` of its target's half-width across, from
//! `least` to `most` millimetres, `more` as wide again for each unit past the first up to
//! `units`; and its light is `ribbon` as wide as its head, drawn `halo` times as wide again
//! because the ribbon sprite fades to nothing at its sides where the web's ribbon has a halo
//! round its body.
//!
//! As the web's `streakLight` (`ui/src/stage/flare.ts`) does, a streak also lights what it
//! left, with a glow `orb` times its head's half-width across in its color with a hot middle,
//! which fades as the head goes, and has a hot point at its head `dot` times its head's
//! half-width across. Its body is the ribbon sprite, the same all along, so that the records
//! along it join into one band; the comet sprite drew a bead at each record.

use beryl_protocol::protocol::Mana;
use beryl_protocol::script::{Stream, Subject};
use glam::{Vec2, Vec3, Vec3Swizzles};

use super::{Lit, StreakRun};
use crate::play::{Reach, Scene};
use kit_base::data::verbs::Streak;
use kit_base::tokens::color;
use kit_frame::frame::{Band, Draw, FrameBuilder, sprites};
use kit_fx::effects::light::quad;

/// How many records draw one streak, evenly along the part of its arc from its tail to its
/// head.
pub const RECORDS: usize = 8;

/// Writes the records of the streaks of mana and damage in `lit` as they are at `now`.
pub fn write(lit: &Lit, now: u64, scene: &Scene<'_>, builder: &mut FrameBuilder<'_>) {
    let tuning = &scene.table().data().verbs.streak;
    let runs = lit.streaks();
    for (index, run) in runs.iter().enumerate() {
        if now < run.over.from || now >= run.over.to {
            continue;
        }
        let (Some(from), Some(target)) = (
            source(run, scene, builder),
            scene.light_target(run.to, builder),
        ) else {
            continue;
        };
        let to = if matches!(run.to, Subject::Player { .. }) {
            Reach {
                half: Vec2::ZERO,
                ..target.reach
            }
        } else {
            target.reach
        };
        let nth = runs[..index]
            .iter()
            .filter(|other| beside(other, run, from, scene, builder))
            .count();
        #[expect(
            clippy::cast_precision_loss,
            reason = "a time inside a step that lasts under a second"
        )]
        let share = (now - run.over.from) as f32 / run.over.duration() as f32;
        let through = run.curve.ease(share);
        // A source rises out of its row toward the middle of the table, the way its controller
        // faces (`Scene::toward`, which is -1 up the screen): the viewer's away from the eye,
        // the opponent's toward it, wherever it lies. A pool faces neither way and rises away,
        // as the web's does (`from.toward || -1` in `cometOf`).
        let faces = match run.from {
            Some(Subject::Player { .. }) | None => 1.0,
            Some(source) => {
                let toward = scene.toward(source);
                if toward == 0.0 { 1.0 } else { -toward }
            }
        };
        let comet = Comet::new(from, to, through, run.amount, nth, faces, tuning);
        let eye = scene.table().camera().eye;
        let tint = tint(run.stream);
        // The glow where it left, in its color with a hot middle, which fades as its head
        // goes: the web's `flash` glint `orb` times the head's width at the curve's start,
        // at `1.1 * (1 - through)^0.7`.
        let orb = 1.1 * (1.0 - through).powf(0.7);
        let source = comet.a;
        for (size, color) in [(tuning.orb, tint), (tuning.orb * 0.5, hot(tint))] {
            let across = 2.0 * size * comet.wide;
            builder.draw(
                Band::Glows,
                Draw::glow(
                    quad(source, Vec3::X, eye, across, across),
                    fade(color, orb),
                    sprites::DISC,
                    0.0,
                ),
            );
        }
        // How far apart the records lie along the lit part of the arc; each reaches half way
        // to its neighbours, so that they overlap.
        let length = comet.length();
        let lit = through - comet.back;
        #[expect(clippy::cast_precision_loss, reason = "8 records")]
        let spacing = length * lit / RECORDS as f32;
        for record in 1..=RECORDS {
            #[expect(clippy::cast_precision_loss, reason = "a record's number, under 9")]
            let share = record as f32 / RECORDS as f32;
            let t = comet.back + lit * share;
            let at = comet.at(t);
            let tangent = comet.tangent(t);
            let half = comet.wide * share * share * tuning.ribbon * tuning.halo;
            let long = (2.0 * spacing).max(2.0 * half);
            // The material lights a record from the tail at `head - tail` to the head at
            // `head`, in units of the record's own length, and fades it from the head back. So
            // that the records together fade evenly along the whole streak, each is given the
            // place of the streak's head and the streak's lit length measured in its own
            // length, from its middle at `t`.
            let own = (long / length.max(f32::EPSILON)).max(f32::EPSILON);
            builder.draw(
                Band::Glows,
                Draw::streak(
                    quad(at, tangent, eye, long, 2.0 * half),
                    tint,
                    sprites::RIBBON,
                    (through - t) / own + 0.5,
                    lit / own,
                ),
            );
        }
        // The hot point at its head (the web's dot `5` times the head's width across).
        let across = tuning.dot * comet.wide;
        builder.draw(
            Band::Glows,
            Draw::glow(
                quad(comet.at(through), Vec3::X, eye, across, across),
                hot(tint),
                sprites::DISC,
                0.0,
            ),
        );
    }
}

/// `tint` most of the way to white, as the hot middle of the web's light (`core` in `TONES`,
/// `ui/src/stage/light.ts`) is.
fn hot([red, green, blue, alpha]: [u8; 4]) -> [u8; 4] {
    let toward = |channel: u8| channel + ((255 - channel) / 8) * 7;
    [toward(red), toward(green), toward(blue), alpha]
}

/// `tint` with its opacity scaled by `by`, from 0 to 1.
fn fade(tint: [u8; 4], by: f32) -> [u8; 4] {
    let [red, green, blue, alpha] = tint;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "an opacity from 0 to 255"
    )]
    let alpha = (f32::from(alpha) * by.clamp(0.0, 1.0)).round() as u8;
    [red, green, blue, alpha]
}

/// Where `run` leaves: its player's pool, or the thing it leaves where that is on the table.
fn source(run: &StreakRun, scene: &Scene<'_>, builder: &FrameBuilder<'_>) -> Option<Reach> {
    match run.from? {
        Subject::Player { player } => Some(Reach {
            pose: scene.table().pool_pose(player),
            half: Vec2::ZERO,
        }),
        from => Some(scene.light_target(from, builder)?.reach),
    }
}

/// Whether `other` left one place for `run`'s target together with `run`: at the same time,
/// for the same target, from within a card's half-width of `run`'s source, as the lands of a
/// pile lie.
fn beside(
    other: &StreakRun,
    run: &StreakRun,
    from: Reach,
    scene: &Scene<'_>,
    builder: &FrameBuilder<'_>,
) -> bool {
    other.over.from == run.over.from
        && other.to == run.to
        && source(other, scene, builder).is_some_and(|theirs| {
            theirs.pose.position.xy().distance(from.pose.position.xy()) <= from.half.x
        })
}

/// The tint of a streak of `stream`.
fn tint(stream: Stream) -> [u8; 4] {
    match stream {
        Stream::Damage => color::DAMAGE,
        Stream::Mana { color: mana } => match mana {
            Mana::White => color::STREAK_WHITE,
            Mana::Blue => color::STREAK_BLUE,
            Mana::Black => color::STREAK_BLACK,
            Mana::Red => color::STREAK_RED,
            Mana::Green => color::STREAK_GREEN,
            Mana::Colorless => color::STREAK_COLORLESS,
        },
    }
}

/// The shape of one streak at one time: the arc from `a` to `b` by way of `c`, how far along
/// it its tail is, and half the width of its head, in millimetres.
struct Comet {
    a: Vec3,
    c: Vec3,
    b: Vec3,
    back: f32,
    wide: f32,
}

impl Comet {
    /// The comet from `from` to `to` with its head `through` of the way there, carrying
    /// `amount` units, the `nth` after the first to leave that place for that target together.
    /// It rises to the side of its way that `faces` says: away from the viewer for 1, toward
    /// them for -1.
    fn new(
        from: Reach,
        to: Reach,
        through: f32,
        amount: u8,
        nth: usize,
        faces: f32,
        tuning: &Streak,
    ) -> Self {
        let (a, b) = (from.pose.position, to.pose.position);
        let way = (b - a).xy();
        let far = way.length();
        // Those of a sheaf rise on either side of the way by turns, each pair higher.
        #[expect(clippy::cast_precision_loss, reason = "a few streaks at once")]
        let pair = (nth / 2) as f32;
        let sheaf = if nth % 2 == 1 { -1.0 } else { 1.0 } * (1.0 + tuning.fan * pair);
        let high = from.half.y.max(to.half.y);
        let rise = (tuning.bow * far).min(tuning.rise * 2.0 * high) * sheaf;
        // The side of the way `faces` says, as the web's `side` picks it: the way's left turned
        // by the sign of how far it goes across the screen, and for a way straight up or down
        // the screen, its left for a source that faces away and its right for one that faces
        // the viewer.
        let across = if way.x == 0.0 { 1.0 } else { way.x.signum() };
        let side = way.perp().normalize_or(Vec2::X) * across * faces;
        let c = (a + b) * 0.5 + (side * rise).extend(0.0);
        let units = f32::from(amount.max(1)).min(tuning.units);
        let wide = (tuning.head * to.half.x).clamp(tuning.least, tuning.most)
            * (1.0 + tuning.more * (units - 1.0));
        // The tail is drawn in as the head comes to land.
        let back = (through - tuning.tail * (1.0 - through.powi(3))).max(0.0);
        Self {
            a,
            c,
            b,
            back,
            wide,
        }
    }

    /// The point `t` of the way along the arc.
    fn at(&self, t: f32) -> Vec3 {
        let u = 1.0 - t;
        self.a * (u * u) + self.c * (2.0 * u * t) + self.b * (t * t)
    }

    /// The way the arc runs at `t`.
    fn tangent(&self, t: f32) -> Vec3 {
        let along = (self.c - self.a) * (2.0 * (1.0 - t)) + (self.b - self.c) * (2.0 * t);
        along.normalize_or(Vec3::X)
    }

    /// The arc's length, near enough: its chord and control polygon averaged.
    fn length(&self) -> f32 {
        let chord = self.a.distance(self.b);
        let polygon = self.a.distance(self.c) + self.c.distance(self.b);
        f32::midpoint(chord, polygon)
    }
}
