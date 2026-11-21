//! Draws the arrows and lines that `Lit` holds (UI Conventions, Verbs, `point`). Each follows
//! its endpoints (`Scene::reach`) along the web stage's curve (`lines::way_of`). If either
//! endpoint has no place in a frame, that frame omits the arrow.
//!
//! Each arrow writes one resolved element, `Role::Arrow`, named by its kind, whose four corners
//! are the control points of its cubic Bezier curve on the screen, and a ribbon of light along
//! the curve, with a head at its end for the two target kinds, in `Band::Glows`. The ribbon is
//! `RIBBON` pieces, each two quads: its body and halo in its tone's color, and a hot core in the
//! tone's core color, as the web's ribbon shader lights them (`ui/src/stage/glow.ts`). Its
//! width along its length follows the web stage's `widthOf` (`ui/src/stage/light.ts`), its
//! opacity the fade of its `Open`, and its color what it says.
//! The values are `light` in `data/beryl3d/space.json`: lengths in millimetres at the arrow's
//! target, which `Camera::points_per_millimetre` turns into points, and shares as they are.

use std::f32::consts::PI;

use beryl_protocol::script::{Arrow, Subject};
use glam::{Vec2, Vec3};

use super::lines::{End, along, cubic, way_of};
use super::marks;
use super::{Lit, Open, Span};
use crate::play::{Reach, Scene};
use crate::verbs::Interval;
use kit_base::data::space::light;
use kit_base::data::space::light::glow;
use kit_base::tokens::color;
pub use kit_frame::frame::ARROW_IDS;
use kit_frame::frame::sprites;
use kit_frame::frame::{Band, Draw, ElementId, FrameBuilder, Place, Resolved, Role, StateBits};

/// How many pieces an arrow's ribbon is cut into, each a quad of its body and halo and a quad of
/// its core. The cuts lie closer together where its width changes faster, so that no piece is
/// much wider than the next.
pub const RIBBON: usize = 20;

/// How many points along its curve a ribbon's cuts are chosen among. They lie closer together
/// toward its end, where an arrow's head flares and a line of combat ends in a dart.
const FINE: usize = 32;

/// How a ribbon is cut along its length (`light.ts` `Cut`): an arrow at a target is a thread at
/// its tail that swells to a spear head; a line of combat a beam that narrows to a dart; the line
/// to a source a hairline that is widest a little past half way and fades out before its end.
#[derive(Clone, Copy)]
enum Cut {
    Arrow,
    Swell,
    Beam,
}

/// The element id of the first arrow to the pointer (`Lit::aims`), past every open span's
/// arrow: each one's id is this plus its index among them.
pub const AIM_ID: u32 = ARROW_IDS + (1 << 16);

/// Writes the records of the arrows and lines in `lit` as they are at `now`, and the arrows to
/// the pointer. While the table shows a `prompt` that asks something other than priority, the
/// line from an ability to its source is hidden, as the web stage's `Overlay.lines` hides it
/// (UI Conventions, "A line from an ability to its source").
pub fn write(lit: &Lit, now: u64, prompt: bool, scene: &Scene<'_>, builder: &mut FrameBuilder<'_>) {
    for (index, open) in lit.open().iter().enumerate() {
        let Span::Arrow(arrow) = open.span else {
            continue;
        };
        if prompt && arrow.arrow == Arrow::Source {
            continue;
        }
        let (Some(from), Some(to)) = (scene.reach(arrow.from), scene.reach(arrow.to)) else {
            continue;
        };
        // A player's end is their life total, an oval (`marks::player_half`), as the web's
        // `reachOfLife` makes it: an arrow at a player stops at its rim, and is sized by it.
        let (from, to) = (life(scene, arrow.from, from), life(scene, arrow.to, to));
        let ends = [(arrow.from, from), (arrow.to, to)].map(|(subject, reach)| End {
            round: matches!(subject, Subject::Player { .. }),
            toward: scene.toward(subject),
            ..on_screen(scene, reach)
        });
        let moving = scene.moving(arrow.from) || scene.moving(arrow.to);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "far fewer open spans than element ids"
        )]
        let id = ARROW_IDS + index as u32;
        let drawn = Drawn {
            arrow: arrow.arrow,
            ends,
            reaches: [from, to],
            opacity: fade(open, now),
            moving,
            id,
        };
        drawn.write(scene, builder);
    }
    for (index, aim) in lit.aims().iter().enumerate() {
        let Some(from) = scene.reach(aim.from) else {
            continue;
        };
        let from = life(scene, aim.from, from);
        let start = End {
            round: matches!(aim.from, Subject::Player { .. }),
            toward: scene.toward(aim.from),
            ..on_screen(scene, from)
        };
        let pointer = End {
            middle: aim.to,
            half: Vec2::ZERO,
            round: false,
            toward: 0.0,
        };
        #[expect(
            clippy::cast_possible_truncation,
            reason = "far fewer arrows to the pointer than element ids"
        )]
        let drawn = Drawn {
            arrow: aim.arrow,
            ends: [start, pointer],
            // The pointer has no size: the arrow is as wide as at what asks.
            reaches: [from, from],
            opacity: 1.0,
            moving: true,
            id: AIM_ID + index as u32,
        };
        drawn.write(scene, builder);
    }
}

/// One arrow as a frame draws it: its kind, its two ends on the screen and the things there,
/// how clear it is, whether an end moves, and its element id.
struct Drawn {
    arrow: Arrow,
    ends: [End; 2],
    reaches: [Reach; 2],
    opacity: f32,
    moving: bool,
    id: u32,
}

impl Drawn {
    /// Writes its element and its ribbon of light, with a head for a target's arrow.
    fn write(&self, scene: &Scene<'_>, builder: &mut FrameBuilder<'_>) {
        let camera = scene.table().camera();
        let [from, to] = self.reaches;
        let sag = light::SAG * camera.window.size.height;
        let Some(way) = way_of(self.arrow, self.ends[0], self.ends[1], Some(sag)) else {
            return;
        };
        let points = cubic(way);
        let opacity = self.opacity;
        builder.resolved(
            self.arrow.name(),
            Resolved {
                id: ElementId(self.id),
                parent: u32::MAX,
                role: Role::Arrow,
                name: kit_frame::frame::NameRef::default(),
                place: Place::Layer,
                pose: None,
                corners: points,
                depth: camera.depth(to.pose.position),
                opacity,
                clip: None,
                interactive: false,
                state: StateBits(if self.moving { StateBits::MOVING } else { 0 }),
                face: None,
                copies: 1,
                really: kit_frame::frame::NameRef::default(),
                note: kit_frame::frame::NameRef::default(),
            },
        );
        let points_per_millimetre = camera.points_per_millimetre(to.pose.position);
        let sized = |reach: &Reach| {
            (reach.half.x.min(reach.half.y) / light::sized::HALF)
                .clamp(light::sized::LEAST, light::sized::MOST)
        };
        let k = sized(&from).min(sized(&to));
        let (cut, wide, tone) = match self.arrow {
            Arrow::FriendlyTarget => (Cut::Arrow, light::wide::TARGET, color::FRIENDLY_TARGET),
            Arrow::HostileTarget => (Cut::Arrow, light::wide::TARGET, color::HOSTILE_TARGET),
            Arrow::Source => (Cut::Swell, light::wide::SOURCE, color::SOURCE_LINE),
            Arrow::Attack => (Cut::Beam, light::wide::ATTACK, color::ATTACK_LINE),
            Arrow::Block => (Cut::Beam, light::wide::BLOCK, color::BLOCK_LINE),
        };
        let (core, gain) = match self.arrow {
            Arrow::FriendlyTarget => (color::FRIENDLY_TARGET_CORE, glow::gains::TARGET),
            Arrow::HostileTarget => (color::HOSTILE_TARGET_CORE, glow::gains::TARGET),
            Arrow::Source => (color::SOURCE_LINE_CORE, glow::gains::SOURCE),
            Arrow::Attack => (color::ATTACK_LINE_CORE, glow::gains::ATTACK),
            Arrow::Block => (color::BLOCK_LINE_CORE, glow::gains::BLOCK),
        };
        let tint = tinted(tone, opacity);
        let shine = Shine {
            tone,
            core,
            // The shapes hold their light at the gain `glow::GAIN`.
            strength: opacity * gain / glow::GAIN,
        };
        // The line to a source grows out of the source, which is where it ends, so it is cut
        // from that end.
        let ribbon = match self.arrow {
            Arrow::Source => [points[3], points[2], points[1], points[0]],
            _ => points,
        };
        let lengths = Lengths {
            wide: wide * k * points_per_millimetre,
            per_millimetre: points_per_millimetre,
        };
        write_ribbon(scene, &ribbon, cut, lengths, shine, builder);
        if matches!(self.arrow, Arrow::FriendlyTarget | Arrow::HostileTarget) {
            write_head(
                scene,
                &points,
                self.ends[1].half.x,
                points_per_millimetre,
                tint,
                builder,
            );
        }
    }
}

/// Where `subject` reaches, `reach`, with a player's anchor given the size of their life total.
fn life(scene: &Scene<'_>, subject: Subject, reach: Reach) -> Reach {
    match subject {
        Subject::Player { .. } => Reach {
            half: marks::player_half(scene, reach.pose.scale),
            ..reach
        },
        Subject::Object { .. } | Subject::Ability { .. } => reach,
    }
}

/// `reach` as an end of a line on the screen: the box round its four corners there, or its middle
/// alone for what has no size.
fn on_screen(scene: &Scene<'_>, reach: Reach) -> End {
    let camera = scene.table().camera();
    if reach.half == Vec2::ZERO || reach.pose.scale <= 0.0 {
        return End {
            middle: camera.to_screen(reach.pose.position),
            half: Vec2::ZERO,
            round: false,
            toward: 0.0,
        };
    }
    let transform = reach.pose.transform();
    let half = reach.half / reach.pose.scale;
    let mut low = Vec2::splat(f32::INFINITY);
    let mut high = Vec2::splat(f32::NEG_INFINITY);
    for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        let corner =
            camera.to_screen(transform.transform_point3(Vec3::new(x * half.x, y * half.y, 0.0)));
        low = low.min(corner);
        high = high.max(corner);
    }
    End {
        middle: (low + high) / 2.0,
        half: (high - low) / 2.0,
        round: false,
        toward: 0.0,
    }
}

/// How whole `open` stands at `now`, from 0 to 1: fading in over its beginning and out over its
/// end.
fn fade(open: &Open, now: u64) -> f32 {
    let shown = |over: Interval| {
        if now >= over.to {
            1.0
        } else if now < over.from {
            0.0
        } else {
            #[expect(
                clippy::cast_precision_loss,
                reason = "a share of a fade that lasts a fraction of a second"
            )]
            let share = (now - over.from) as f32 / over.duration() as f32;
            share
        }
    };
    let begun = shown(open.begins);
    let ended = open.ends.map_or(0.0, shown);
    begun * (1.0 - ended)
}

/// `tone` at `opacity`.
fn tinted(tone: [u8; 4], opacity: f32) -> [u8; 4] {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "an opacity from 0 to 1 of a byte"
    )]
    let alpha = (f32::from(tone[3]) * opacity.clamp(0.0, 1.0)).round() as u8;
    [tone[0], tone[1], tone[2], alpha]
}

/// A ribbon's widest half-width and a millimetre, in points on the screen.
#[derive(Clone, Copy)]
struct Lengths {
    wide: f32,
    per_millimetre: f32,
}

/// How a ribbon is lit: the color of its body and halo, the color of its core, and how strongly,
/// as a share of the light its shapes hold.
#[derive(Clone, Copy)]
struct Shine {
    tone: [u8; 4],
    core: [u8; 4],
    strength: f32,
}

/// Writes the `RIBBON` pieces of a ribbon along the curve `points` (`cut_ribbon`), each a quad
/// of the ribbon's body and halo, as high as the web's strip of triangles is there
/// (`2.6 b + 16` web pixels each side of a body `b` wide), and a quad of its core over the body.
/// Each piece narrows from the ribbon's width at one cut to its width at the next
/// (`Draw::tapered`), and its ends lie across the line that halves the turn there
/// (`Draw::mitred`), so the ribbon has no step in width and no gap where it bends. The core of
/// an arrow at a target lies a little up the screen from its middle, as the web's does, as if
/// lit from one side.
fn write_ribbon(
    scene: &Scene<'_>,
    points: &[Vec2; 4],
    cut: Cut,
    lengths: Lengths,
    shine: Shine,
    builder: &mut FrameBuilder<'_>,
) {
    let Some((cuts, whole)) = cut_ribbon(points, cut, lengths) else {
        return;
    };
    let camera = scene.table().camera();
    let pixel = glow::PIXEL * lengths.per_millimetre;
    // The side the core leans to, as the web's `flip`: up the screen for a ribbon that runs
    // right, judged where the ribbon's middle is.
    let middle = (cuts[RIBBON / 2].0, cuts[RIBBON / 2 + 1].0);
    let flip = if middle.1.x - middle.0.x < 0.0 {
        -1.0
    } else {
        1.0
    };
    let lean = match cut {
        Cut::Arrow => glow::CORE_LEAN,
        Cut::Swell | Cut::Beam => 0.0,
    };
    // Each cut: where it is, the body's half-width there, how high the halo reaches each side,
    // and how high the core. The web fades its ribbon in over its first pixels and its halo out
    // over its last few. A record has one opacity, which would step from piece to piece, so
    // here the ends narrow instead, which the pieces' tapers carry on smoothly: the core over
    // the web's fade, and the halo, with the body inside it, to a point over its last
    // `REACH_PIXELS` at each end. The halo's profile is the one its unnarrowed height gives.
    let reach_pixels = glow::REACH_PIXELS * pixel;
    let cuttings = cuts.map(|(at, arc, half)| {
        let reach = glow::REACH_SHARE * half + reach_pixels;
        let narrowed = smooth(0.0, reach_pixels, arc) * smooth(0.0, reach_pixels, whole - arc);
        Cutting {
            at,
            half,
            column: sprites::ribbon_lit_column(half / reach),
            reach: reach * narrowed.max(0.02),
            core: half * smooth(0.0, glow::FADE_PIXELS * pixel, arc),
            halves: Vec2::ZERO,
        }
    });
    // The way each piece runs, and at each cut the way that halves the turn there.
    let mut ways = [Vec2::X; RIBBON];
    for (way, pair) in ways.iter_mut().zip(cuttings.windows(2)) {
        *way = (pair[1].at - pair[0].at).normalize_or(Vec2::X);
    }
    let mut cuttings = cuttings;
    for (number, cutting) in cuttings.iter_mut().enumerate() {
        let before = ways[number.saturating_sub(1)];
        let after = ways[number.min(RIBBON - 1)];
        cutting.halves = (before + after).normalize_or(after);
    }
    for (number, pair) in cuttings.windows(2).enumerate() {
        let (from, to) = (pair[0], pair[1]);
        let long = from.at.distance(to.at);
        if long <= 0.0 {
            continue;
        }
        let way = ways[number];
        let centre = (from.at + to.at) / 2.0;
        // The quad's body runs from one column of `ribbon_lit` to another along its length,
        // and the columns must rise along the quad: where they fall, the quad is turned round,
        // which the halo and body, alike on both sides, do not show.
        let (near, far, along) = if from.column <= to.column {
            (from, to, way)
        } else {
            (to, from, -way)
        };
        let reach = near.reach.max(far.reach);
        builder.draw(
            Band::Glows,
            Draw::glow(
                camera.screen_transform(centre, Vec2::new(long, 2.0 * reach), along),
                tinted(shine.tone, shine.strength),
                sprites::ribbon_lit(near.column, far.column),
                0.0,
            )
            .tapered(near.reach / reach, far.reach / reach)
            .mitred(slant(along, near.halves), slant(along, far.halves)),
        );
        let high = from.core.max(to.core);
        if high * glow::CORE_REACH < 0.05 {
            continue;
        }
        let up = Vec2::new(way.y, -way.x) * flip;
        let lean_by = lean * (from.half + to.half) / 2.0;
        builder.draw(
            Band::Glows,
            Draw::glow(
                camera.screen_transform(
                    centre + up * lean_by,
                    Vec2::new(long, 2.0 * high * glow::CORE_REACH),
                    way,
                ),
                tinted(shine.core, shine.strength),
                sprites::RIBBON_CORE,
                0.0,
            )
            .tapered(from.core / high, to.core / high)
            .mitred(slant(way, from.halves), slant(way, to.halves)),
        );
    }
}

/// A point of a ribbon: where it is, how far along its curve, and its half-width there.
type Sample = (Vec2, f32, f32);

/// Where the `RIBBON + 1` cuts between the pieces of a ribbon along the curve `points` lie,
/// each with how far along the curve it is and the ribbon's half-width there (between those of
/// the fine points beside it), and how long the curve is; `None` for a curve of no length. The cuts are chosen among `FINE` points along the curve so that each piece takes an
/// equal share of the ribbon's length, of how much its width changes and of how far it turns,
/// weighed by `glow::WIDTH_WEIGHT` and `glow::TURN_WEIGHT`: where an arrow flares near its head
/// or bends most, the pieces are short.
fn cut_ribbon(
    points: &[Vec2; 4],
    cut: Cut,
    lengths: Lengths,
) -> Option<([Sample; RIBBON + 1], f32)> {
    // Each fine point: where it is, how far along the curve, and the half-width there.
    let mut fine: [Sample; FINE + 1] = [(Vec2::ZERO, 0.0, 0.0); FINE + 1];
    let mut arc = 0.0;
    for number in 0..=FINE {
        #[expect(clippy::cast_precision_loss, reason = "a small count")]
        let before_end = 1.0 - number as f32 / FINE as f32;
        let at = along(points, 1.0 - before_end * before_end);
        if number > 0 {
            arc += fine[number - 1].0.distance(at);
        }
        fine[number] = (at, arc, 0.0);
    }
    let whole = arc;
    if whole <= 0.0 {
        return None;
    }
    let mut widest = 0.0_f32;
    for point in &mut fine {
        point.2 = width_of(cut, lengths, point.1, whole);
        widest = widest.max(point.2);
    }
    // How far along each fine point is, in shares of length, of change of width and of turn
    // (in radians) together.
    let mut measure = [0.0_f32; FINE + 1];
    for number in 1..=FINE {
        let (before, here) = (fine[number - 1], fine[number]);
        let changed = (here.2 - before.2).abs() / widest.max(f32::EPSILON);
        // The sine of the turn, which is near enough to its angle for the small turns
        // between fine points.
        let turned = if number >= 2 {
            let earlier = (before.0 - fine[number - 2].0).normalize_or_zero();
            let later = (here.0 - before.0).normalize_or_zero();
            earlier.perp_dot(later).abs()
        } else {
            0.0
        };
        measure[number] = measure[number - 1]
            + (here.1 - before.1) / whole
            + glow::WIDTH_WEIGHT * changed
            + glow::TURN_WEIGHT * turned;
    }
    let mut cuts: [Sample; RIBBON + 1] = [(Vec2::ZERO, 0.0, 0.0); RIBBON + 1];
    let mut below = 0;
    for (number, cut_at) in cuts.iter_mut().enumerate() {
        #[expect(clippy::cast_precision_loss, reason = "a small count")]
        let goal = measure[FINE] * number as f32 / RIBBON as f32;
        while below < FINE - 1 && measure[below + 1] < goal {
            below += 1;
        }
        let span = measure[below + 1] - measure[below];
        let share = if span > 0.0 {
            ((goal - measure[below]) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (from, to) = (fine[below], fine[below + 1]);
        *cut_at = (
            from.0.lerp(to.0, share),
            from.1 + (to.1 - from.1) * share,
            from.2 + (to.2 - from.2) * share,
        );
    }
    Some((cuts, whole))
}

/// How far an end of a quad that runs along `along` moves along it, for each length across it,
/// to lie across `halves`, the way that halves the turn between two pieces of a ribbon: a
/// quad's local y runs `(along.y, -along.x)` on the screen (`Camera::screen_transform`).
fn slant(along: Vec2, halves: Vec2) -> f32 {
    let across = Vec2::new(along.y, -along.x);
    let facing = along.dot(halves);
    if facing.abs() < 1e-3 {
        return 0.0;
    }
    -across.dot(halves) / facing
}

/// A cut between two pieces of a ribbon, as `write_ribbon` draws the pieces on each side of it.
#[derive(Clone, Copy)]
struct Cutting {
    at: Vec2,
    half: f32,
    /// The column boundary of `sprites::ribbon_lit` its body's share of the halo's height
    /// gives.
    column: u16,
    reach: f32,
    /// The body half-width the core is drawn for: the body's, narrowed where the ribbon fades in.
    core: f32,
    /// The way that halves the turn between the pieces on each side of it.
    halves: Vec2,
}

/// 0 at `from` and below, 1 at `to` and above, rising smoothly between, as GLSL's `smoothstep`.
fn smooth(from: f32, to: f32, t: f32) -> f32 {
    if to <= from {
        return if t >= to { 1.0 } else { 0.0 };
    }
    let t = ((t - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Writes the head of an arrow at a target: its tip where the curve `points` ends, lying along
/// the way it arrives, as long as a share of `target_half`, half the width of what it meets on
/// the screen.
fn write_head(
    scene: &Scene<'_>,
    points: &[Vec2; 4],
    target_half: f32,
    points_per_millimetre: f32,
    tint: [u8; 4],
    builder: &mut FrameBuilder<'_>,
) {
    let long = (light::head::SHARE * target_half).clamp(
        light::head::LEAST * points_per_millimetre,
        light::head::MOST * points_per_millimetre,
    );
    let tip = points[3];
    let way = (tip - points[2]).normalize_or(Vec2::X);
    let transform = scene.table().camera().screen_transform(
        tip - way * (long / 2.0),
        Vec2::new(long, 2.0 * long * light::head::SPREAD),
        way,
    );
    builder.draw(Band::Glows, Draw::glow(transform, tint, sprites::HEAD, 0.0));
}

/// A ribbon's half-width where it has come `arc` of the `whole` points it is long, as the web
/// stage's `widthOf` has it.
fn width_of(cut: Cut, lengths: Lengths, arc: f32, whole: f32) -> f32 {
    let wide = lengths.wide;
    let s = if whole > 0.0 { arc / whole } else { 0.0 };
    let left = whole - arc;
    match cut {
        Cut::Swell => {
            use light::swell::{END, LEAST, PEAK, START};
            let bulge = (PI * s.powf(0.5f32.ln() / PEAK.ln())).sin();
            let ends = (s / START).clamp(0.0, 1.0) * ((1.0 - s) / END).clamp(0.0, 1.0).powf(1.5);
            wide * (LEAST + (1.0 - LEAST) * bulge) * ends
        }
        Cut::Beam => {
            use light::beam::{DART, FROM, HEAD, NECK, TO};
            let (head, neck) = (HEAD * wide, NECK * wide);
            let beam = (FROM + (TO - FROM) * s) * wide;
            if left < head {
                DART * wide * left / head
            } else if left < neck {
                DART * wide + (beam - DART * wide) * (left - head) / (neck - head)
            } else {
                beam
            }
        }
        Cut::Arrow => {
            use light::arrow::{FADE, FADE_POWER, FLARE, FLARE_AT, FLARE_SPREAD, RAMP, RISE, TAIL};
            let per = lengths.per_millimetre;
            let ramp = (s / RAMP).clamp(0.0, 1.0);
            let smooth = ramp * ramp * (3.0 - 2.0 * ramp);
            let tail = TAIL + (1.0 - TAIL) * smooth.powf(RISE);
            let flare =
                1.0 + FLARE * (-((left - FLARE_AT * per) / (FLARE_SPREAD * per)).powi(2)).exp();
            wide * tail * flare * (left / (FADE * per)).clamp(0.0, 1.0).powf(FADE_POWER)
        }
    }
}
