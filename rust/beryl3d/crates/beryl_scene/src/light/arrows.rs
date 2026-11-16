//! Draws the arrows and lines that `Lit` holds (UI Conventions, Verbs, `point`). Each is drawn on
//! every frame from where its two ends are then (`Scene::reach`), so it is never anywhere they
//! are not: its shape is the web stage's (`lines::way_of`), on the screen, and an arrow with an
//! end that has no place this frame is not drawn.
//!
//! Each arrow writes one resolved element, `Role::Arrow`, named by its kind, whose four corners
//! are the control points of its cubic Bezier curve on the screen, and a ribbon of `RIBBON`
//! quads of light along the curve, with a head at its end for the two target kinds, in
//! `Band::Glows`. Its width along its length follows the web stage's `widthOf`
//! (`ui/src/stage/light.ts`), its opacity the fade of its `Open`, and its color what it says.
//! The values are `light` in `data/beryl3d/space.json`: lengths in millimetres at the arrow's
//! target, which `Camera::points_per_millimetre` turns into points, and shares as they are.

use std::f32::consts::PI;

use beryl_protocol::script::Arrow;
use glam::{Vec2, Vec3};

use super::lines::{End, along, cubic, way_of};
use super::{Lit, Open, Span};
use crate::data::space::light;
use crate::frame::sprites;
use crate::frame::{Band, Draw, ElementId, FrameBuilder, Place, Resolved, Role, StateBits};
use crate::play::{Reach, Scene};
use crate::tokens::color;
use crate::verbs::Interval;

/// How many quads of light an arrow's ribbon is cut into.
pub const RIBBON: usize = 16;

/// The first element id of the arrows: an arrow's id is this plus its index among the open
/// spans.
pub const ARROW_IDS: u32 = 1 << 22;

/// How a ribbon is cut along its length (`light.ts` `Cut`): an arrow at a target is a thread at
/// its tail that swells to a spear head; a line of combat a beam that narrows to a dart; the line
/// to a source a hairline that is widest a little past half way and fades out before its end.
#[derive(Clone, Copy)]
enum Cut {
    Arrow,
    Swell,
    Beam,
}

/// Writes the records of the arrows and lines in `lit` as they are at `now`.
pub fn write(lit: &Lit, now: u64, scene: &Scene<'_>, builder: &mut FrameBuilder<'_>) {
    for (index, open) in lit.open().iter().enumerate() {
        let Span::Arrow(arrow) = open.span else {
            continue;
        };
        let (Some(from), Some(to)) = (scene.reach(arrow.from), scene.reach(arrow.to)) else {
            continue;
        };
        let camera = scene.table().camera();
        let ends = [(arrow.from, from), (arrow.to, to)].map(|(subject, reach)| End {
            toward: scene.toward(subject),
            ..on_screen(scene, reach)
        });
        let sag = light::SAG * camera.window.size.height;
        let Some(way) = way_of(arrow.arrow, ends[0], ends[1], Some(sag)) else {
            continue;
        };
        let points = cubic(way);
        let moving = scene.moving(arrow.from) || scene.moving(arrow.to);
        let opacity = fade(open, now);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "far fewer open spans than element ids"
        )]
        let id = ARROW_IDS + index as u32;
        builder.resolved(
            arrow.arrow.name(),
            Resolved {
                id: ElementId(id),
                parent: u32::MAX,
                role: Role::Arrow,
                name: crate::frame::NameRef::default(),
                place: Place::Layer,
                corners: points,
                depth: camera.depth(to.pose.position),
                opacity,
                clip: None,
                interactive: false,
                state: StateBits(if moving { StateBits::MOVING } else { 0 }),
                face: None,
                really: crate::frame::NameRef::default(),
            },
        );
        let points_per_millimetre = camera.points_per_millimetre(to.pose.position);
        let sized = |reach: &Reach| {
            (reach.half.x.min(reach.half.y) / light::sized::HALF)
                .clamp(light::sized::LEAST, light::sized::MOST)
        };
        let k = sized(&from).min(sized(&to));
        let (cut, wide, tone) = match arrow.arrow {
            Arrow::FriendlyTarget => (Cut::Arrow, light::wide::TARGET, color::FRIENDLY_TARGET),
            Arrow::HostileTarget => (Cut::Arrow, light::wide::TARGET, color::HOSTILE_TARGET),
            Arrow::Source => (Cut::Swell, light::wide::SOURCE, color::SOURCE_LINE),
            Arrow::Attack => (Cut::Beam, light::wide::ATTACK, color::ATTACK_LINE),
            Arrow::Block => (Cut::Beam, light::wide::BLOCK, color::BLOCK_LINE),
        };
        let tint = tinted(tone, opacity);
        // The line to a source grows out of the source, which is where it ends, so it is cut
        // from that end.
        let ribbon = match arrow.arrow {
            Arrow::Source => [points[3], points[2], points[1], points[0]],
            _ => points,
        };
        let lengths = Lengths {
            wide: wide * k * points_per_millimetre,
            per_millimetre: points_per_millimetre,
        };
        write_ribbon(scene, &ribbon, cut, lengths, tint, builder);
        if matches!(arrow.arrow, Arrow::FriendlyTarget | Arrow::HostileTarget) {
            write_head(
                scene,
                &points,
                ends[1].half.x,
                points_per_millimetre,
                tint,
                builder,
            );
        }
    }
}

/// `reach` as an end of a line on the screen: the box round its four corners there, or its middle
/// alone for a player's anchor, which has no size.
fn on_screen(scene: &Scene<'_>, reach: Reach) -> End {
    let camera = scene.table().camera();
    if reach.half == Vec2::ZERO || reach.pose.scale <= 0.0 {
        return End {
            middle: camera.to_screen(reach.pose.position),
            half: Vec2::ZERO,
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

/// Writes `RIBBON` quads of light along the curve `points`, each as wide as `width_of` has the
/// ribbon where its middle lies.
fn write_ribbon(
    scene: &Scene<'_>,
    points: &[Vec2; 4],
    cut: Cut,
    lengths: Lengths,
    tint: [u8; 4],
    builder: &mut FrameBuilder<'_>,
) {
    let mut samples = [Vec2::ZERO; RIBBON + 1];
    for (number, sample) in samples.iter_mut().enumerate() {
        #[expect(clippy::cast_precision_loss, reason = "a small count")]
        let t = number as f32 / RIBBON as f32;
        *sample = along(points, t);
    }
    let whole: f32 = samples
        .windows(2)
        .map(|pair| pair[0].distance(pair[1]))
        .sum();
    let camera = scene.table().camera();
    let mut arc = 0.0;
    for pair in samples.windows(2) {
        let long = pair[0].distance(pair[1]);
        let half = width_of(cut, lengths, arc + long / 2.0, whole);
        arc += long;
        let way = (pair[1] - pair[0]).normalize_or(Vec2::X);
        let transform =
            camera.screen_transform((pair[0] + pair[1]) / 2.0, Vec2::new(long, 2.0 * half), way);
        builder.draw(
            Band::Glows,
            Draw::glow(transform, tint, sprites::RIBBON, 1.0),
        );
    }
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
    builder.draw(Band::Glows, Draw::glow(transform, tint, sprites::HEAD, 1.0));
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
