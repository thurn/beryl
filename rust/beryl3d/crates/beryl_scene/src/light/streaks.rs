//! Draws the streaks of mana and damage that `Lit` holds, as the web stage's `cometOf`
//! (`ui/src/stage/streaks.ts`) shapes them: a bright head on a low arc from its source to its
//! target, with a tail that stretches out of the source, follows, and is drawn in as the head
//! lands. A streak is drawn while its step plays and at no other time.
//!
//! Both ends are read from the scene on each frame, so that a streak to a card in flight lands
//! on it where it is. A streak from a player leaves their pool's anchor. A streak whose source
//! or target has no place on the table is not drawn.
//!
//! The values are `verbs.streak` in `data/beryl3d/space.json`, the web's `COMET` and
//! `STREAK.ribbon` with its stage pixels in millimetres: the arc rises `bow` of the way between
//! its ends and no more than `rise` of the taller end's height; each pair of streaks after the
//! first two that leave one place for one target together rises `fan` as high again; the tail
//! is `tail` of the way long; the head is `head` of its target's half-width across, from
//! `least` to `most` millimetres, `more` as wide again for each unit past the first up to
//! `units`; and its light is `ribbon` as wide as its head.

use beryl_protocol::protocol::Mana;
use beryl_protocol::script::{Stream, Subject};
use glam::{Affine3A, Vec2, Vec3, Vec3Swizzles};

use super::{Lit, StreakRun};
use crate::data::space::verbs::streak;
use crate::frame::{Band, Draw, FrameBuilder, sprites};
use crate::play::{Reach, Scene};
use crate::tokens::color;

/// How many records draw one streak, evenly along the part of its arc from its tail to its
/// head.
pub const RECORDS: usize = 8;

/// Writes the records of the streaks of mana and damage in `lit` as they are at `now`.
pub fn write(lit: &Lit, now: u64, scene: &Scene<'_>, builder: &mut FrameBuilder<'_>) {
    let runs = lit.streaks();
    for (index, run) in runs.iter().enumerate() {
        if now < run.over.from || now >= run.over.to {
            continue;
        }
        let (Some(from), Some(to)) = (source(run, scene), scene.reach(run.to)) else {
            continue;
        };
        let nth = runs[..index]
            .iter()
            .filter(|other| beside(other, run, from, scene))
            .count();
        #[expect(
            clippy::cast_precision_loss,
            reason = "a time inside a step that lasts under a second"
        )]
        let share = (now - run.over.from) as f32 / run.over.duration() as f32;
        let through = run.curve.ease(share);
        // A source rises out of its row toward the middle of the table: one on the viewer's
        // side away from the eye, one on the opponent's toward it. A pool faces neither way and
        // rises away, as the web's does.
        let faces = match run.from {
            Some(Subject::Player { .. }) => 1.0,
            _ if from.pose.position.y > 0.0 => -1.0,
            _ => 1.0,
        };
        let comet = Comet::new(from, to, through, run.amount, nth, faces);
        let eye = scene.table().camera().eye;
        let tint = tint(run.stream);
        // How far apart the records lie along the lit part of the arc; each reaches half way
        // to its neighbours, so that they overlap.
        #[expect(clippy::cast_precision_loss, reason = "8 records")]
        let spacing = comet.length() * (through - comet.back) / RECORDS as f32;
        for record in 1..=RECORDS {
            #[expect(clippy::cast_precision_loss, reason = "a record's number, under 9")]
            let share = record as f32 / RECORDS as f32;
            let t = comet.back + (through - comet.back) * share;
            let at = comet.at(t);
            let tangent = comet.tangent(t);
            let half = comet.wide * share * share * streak::RIBBON;
            let transform = quad(
                at,
                tangent,
                eye,
                (2.0 * spacing).max(2.0 * half),
                2.0 * half,
            );
            builder.draw(
                Band::Glows,
                Draw::streak(
                    transform,
                    tint,
                    sprites::COMET,
                    through,
                    through - comet.back,
                ),
            );
        }
    }
}

/// Where `run` leaves: its player's pool, or the thing it leaves where that is on the table.
fn source(run: &StreakRun, scene: &Scene<'_>) -> Option<Reach> {
    match run.from? {
        Subject::Player { player } => Some(Reach {
            pose: scene.table().pool_pose(player),
            half: Vec2::ZERO,
        }),
        from => scene.reach(from),
    }
}

/// Whether `other` left one place for `run`'s target together with `run`: at the same time,
/// for the same target, from within a card's half-width of `run`'s source, as the lands of a
/// pile lie.
fn beside(other: &StreakRun, run: &StreakRun, from: Reach, scene: &Scene<'_>) -> bool {
    other.over.from == run.over.from
        && other.to == run.to
        && source(other, scene).is_some_and(|theirs| {
            theirs.pose.position.xy().distance(from.pose.position.xy()) <= from.half.x
        })
}

/// The tint of a streak of `stream`.
fn tint(stream: Stream) -> [u8; 4] {
    match stream {
        Stream::Damage => color::DAMAGE,
        Stream::Mana { color: mana } => match mana {
            Mana::White => color::MANA_WHITE,
            Mana::Blue => color::MANA_BLUE,
            Mana::Black => color::MANA_BLACK,
            Mana::Red => color::MANA_RED,
            Mana::Green => color::MANA_GREEN,
            Mana::Colorless => color::MANA_COLORLESS,
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
    fn new(from: Reach, to: Reach, through: f32, amount: u8, nth: usize, faces: f32) -> Self {
        let (a, b) = (from.pose.position, to.pose.position);
        let way = (b - a).xy();
        let far = way.length();
        // Those of a sheaf rise on either side of the way by turns, each pair higher.
        #[expect(clippy::cast_precision_loss, reason = "a few streaks at once")]
        let pair = (nth / 2) as f32;
        let sheaf = if nth % 2 == 1 { -1.0 } else { 1.0 } * (1.0 + streak::FAN * pair);
        let high = from.half.y.max(to.half.y);
        let rise = (streak::BOW * far).min(streak::RISE * 2.0 * high) * sheaf;
        // For a way straight across the table, the side to its right.
        let mut side = way.perp().normalize_or(Vec2::X);
        if side.y * faces < 0.0 || (side.y == 0.0 && side.x < 0.0) {
            side = -side;
        }
        let c = (a + b) * 0.5 + (side * rise).extend(0.0);
        let units = f32::from(amount.max(1)).min(streak::UNITS);
        let wide = (streak::HEAD * to.half.x).clamp(streak::LEAST, streak::MOST)
            * (1.0 + streak::MORE * (units - 1.0));
        // The tail is drawn in as the head comes to land.
        let back = (through - streak::TAIL * (1.0 - through.powi(3))).max(0.0);
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

/// A sprite's quad at `at`, `length` long along `tangent` and `width` across it, facing `eye`.
pub(crate) fn quad(at: Vec3, tangent: Vec3, eye: Vec3, length: f32, width: f32) -> Affine3A {
    let facing = (eye - at).normalize_or(Vec3::Z);
    let across = facing.cross(tangent).normalize_or(Vec3::Y);
    let normal = tangent.cross(across);
    Affine3A::from_cols(
        (tangent * length).into(),
        (across * width).into(),
        normal.into(),
        at.into(),
    )
}
