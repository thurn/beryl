//! Draws the arrows and lines that `Lit` holds (UI Conventions, Verbs, `point`). Each follows
//! its endpoints (`Scene::reach`) along the web stage's curve (`lines::way_of`). If either
//! endpoint has no place in a frame, that frame omits the arrow.
//!
//! Each arrow writes one resolved element, `Role::Arrow`, named by its kind, whose four corners
//! are the control points of its cubic Bezier curve on the screen, and a ribbon of light along
//! the curve in the overlay band. The two target kinds taper to a point. The ribbon is
//! `RIBBON` pieces, each two quads: its body and halo in its tone's color, and a hot core in the
//! tone's core color, as the web's ribbon shader lights them (`ui/src/stage/glow.ts`). Its
//! width along its length follows the web stage's `widthOf` (`ui/src/stage/light.ts`), its
//! opacity the fade of its `Open`, and its color what it says.
//! Ribbon lengths in `data/beryl3d/light.json` are in reference screen points. The table's
//! screen projection scales them once, regardless of camera depth or display density.

use std::f32::consts::PI;

use beryl_protocol::script::{Arrow, Subject};
use glam::Vec2;

use super::lines::{End, aim_way, along, cubic, way_of};
use super::{Lit, Open, Span};
use crate::play::Scene;
use crate::verbs::Interval;
use kit_base::data::light::Light;
use kit_base::tokens::color;
pub use kit_frame::frame::ARROW_IDS;
use kit_frame::frame::{
    Band, Draw, ElementId, Endpoint, FrameBuilder, LineEnd, Place, Resolved, Role, ShapeAnchor,
    StateBits,
};
use kit_frame::frame::{Sprite, sprites};
use kit_space::camera::Camera;

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
        let (Some(from), Some(to)) = (
            scene.line_target(arrow.from, builder),
            scene.line_target(arrow.to, builder),
        ) else {
            continue;
        };
        let ends = [(arrow.from, from), (arrow.to, to)].map(|(subject, target)| End {
            round: matches!(subject, Subject::Player { .. }),
            toward: scene.toward(subject),
            middle: target.bounds.centre,
            half: target.bounds.half,
        });
        #[expect(
            clippy::cast_possible_truncation,
            reason = "far fewer open spans than element ids"
        )]
        let id = ARROW_IDS + index as u32;
        let drawn = Drawn {
            arrow: arrow.arrow,
            ends,
            targets: [Endpoint::Node(from.id), Endpoint::Node(to.id)],
            depth: scene.table().camera().depth(to.reach.pose.position),
            opacity: fade(open, now),
            moving: from.moving || to.moving,
            id,
        };
        drawn.write(scene, builder);
    }
    for (index, aim) in lit.aims().iter().enumerate() {
        let Some(from) = scene.line_target(aim.from, builder) else {
            continue;
        };
        let start = End {
            round: matches!(aim.from, Subject::Player { .. }),
            toward: scene.toward(aim.from),
            middle: from.bounds.centre,
            half: from.bounds.half,
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
            targets: [Endpoint::Node(from.id), Endpoint::Pointer(aim.to)],
            depth: scene.table().camera().depth(from.reach.pose.position),
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
    targets: [Endpoint; 2],
    depth: f32,
    opacity: f32,
    moving: bool,
    id: u32,
}

impl Drawn {
    /// Writes its element and its ribbon of light.
    fn write(&self, scene: &Scene<'_>, builder: &mut FrameBuilder<'_>) {
        let table = scene.table();
        let light = &table.data().light;
        let profile = table.profile();
        // The desktop fits a reference stage. The phone lays the stage out
        // directly in the board's logical points.
        let scale = if profile.data().table.anchored.is_some() {
            profile.screen_projection().scale
        } else {
            1.0
        };
        let sag = light.sag * table.camera().window.size.height;
        let [from, to] = self.ends;
        let aiming = matches!(self.targets[1], Endpoint::Pointer(_));
        let way = if aiming {
            aim_way(light, self.arrow, from, to.middle, scale)
        } else {
            way_of(light, self.arrow, from, to, Some(sag))
        };
        let Some(way) = way else {
            return;
        };
        let points = cubic(way);
        let opacity = self.opacity;
        let index = builder.resolved(
            self.arrow.name(),
            Resolved {
                id: ElementId(self.id),
                parent: u32::MAX,
                role: Role::Arrow,
                name: kit_frame::frame::NameRef::default(),
                place: Place::Layer,
                pose: None,
                corners: points,
                depth: self.depth,
                opacity,
                clip: None,
                interactive: false,
                state: StateBits(if self.moving { StateBits::MOVING } else { 0 }),
                face: None,
                count: 1,
                annotations: Default::default(),
            },
        );
        builder.decorate(
            index,
            ShapeAnchor::Line([
                LineEnd {
                    target: self.targets[0],
                    offset: points[0] - self.ends[0].middle,
                },
                LineEnd {
                    target: self.targets[1],
                    offset: points[3] - self.ends[1].middle,
                },
            ]),
        );
        // Target and reference-card sizes are both screen lengths. A pointer has no
        // size, so the source alone sets the width of a line to it.
        let sized = |end: End| {
            (end.half.min_element() / (light.sized.half * scale))
                .clamp(light.sized.least, light.sized.most)
        };
        let k = if aiming {
            sized(from)
        } else {
            sized(from).min(sized(to))
        };
        let (cut, wide, shine) = ribbon_style(light, self.arrow, aiming, opacity);
        // The line to a source grows out of the source, which is where it ends, so it is cut
        // from that end.
        let ribbon = match self.arrow {
            Arrow::Source => [points[3], points[2], points[1], points[0]],
            _ => points,
        };
        let lengths = Lengths {
            wide: wide * k * scale,
            scale,
            light,
        };
        let band = builder.node_band(index).records(Band::Glows);
        write_ribbon(scene, &ribbon, cut, lengths, (shine, band), builder);
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

/// A ribbon's widest half-width and the scale of a reference point, in window points.
#[derive(Clone, Copy)]
struct Lengths<'a> {
    wide: f32,
    scale: f32,
    light: &'a Light,
}

/// How a ribbon is lit: the color of its body and halo, the color of its core, and how strongly,
/// as a share of the light its shapes hold.
#[derive(Clone, Copy)]
struct Shine {
    tone: [u8; 4],
    core: [u8; 4],
    strength: f32,
}

/// The cut, reference width and light for a preview or confirmed arrow.
fn ribbon_style(light: &Light, arrow: Arrow, aiming: bool, opacity: f32) -> (Cut, f32, Shine) {
    let (cut, wide, tone, core, gain) = match arrow {
        Arrow::FriendlyTarget | Arrow::HostileTarget if aiming => (
            Cut::Arrow,
            light.wide.aim,
            color::SOURCE_LINE,
            color::SOURCE_LINE_CORE,
            light.glow.gains.aim,
        ),
        Arrow::FriendlyTarget => (
            Cut::Arrow,
            light.wide.target,
            color::FRIENDLY_TARGET,
            color::FRIENDLY_TARGET_CORE,
            light.glow.gains.target,
        ),
        Arrow::HostileTarget => (
            Cut::Arrow,
            light.wide.target,
            color::HOSTILE_TARGET,
            color::HOSTILE_TARGET_CORE,
            light.glow.gains.target,
        ),
        Arrow::Source => (
            Cut::Swell,
            light.wide.source,
            color::SOURCE_LINE,
            color::SOURCE_LINE_CORE,
            light.glow.gains.source,
        ),
        Arrow::Attack => (
            Cut::Beam,
            light.wide.attack,
            color::ATTACK_LINE,
            color::ATTACK_LINE_CORE,
            light.glow.gains.attack,
        ),
        Arrow::Block => (
            Cut::Beam,
            light.wide.block,
            color::BLOCK_LINE,
            color::BLOCK_LINE_CORE,
            light.glow.gains.block,
        ),
    };
    let shine = Shine {
        tone,
        core,
        // The shapes hold their light at the gain `light.glow.gain`.
        strength: opacity * gain / light.glow.gain,
    };
    (cut, wide, shine)
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
    lengths: Lengths<'_>,
    (shine, band): (Shine, Band),
    builder: &mut FrameBuilder<'_>,
) {
    let Some((cuts, whole)) = cut_ribbon(points, cut, lengths) else {
        return;
    };
    let camera = scene.table().camera();
    let light = lengths.light;
    let pixel = lengths.scale;
    // The side the core leans to, as the web's `flip`: up the screen for a ribbon that runs
    // right, judged where the ribbon's middle is.
    let middle = (cuts[RIBBON / 2].0, cuts[RIBBON / 2 + 1].0);
    let flip = if middle.1.x - middle.0.x < 0.0 {
        -1.0
    } else {
        1.0
    };
    let lean = match cut {
        Cut::Arrow => light.glow.core_lean,
        Cut::Swell | Cut::Beam => 0.0,
    };
    let reach_pixels = light.glow.reach_pixels * pixel;
    let body_sections = sections(cuts.map(|(at, arc, half)| {
        let reach = light.glow.reach_share * half + reach_pixels;
        let narrowed = smooth(0.0, reach_pixels, arc) * smooth(0.0, reach_pixels, whole - arc);
        (at, reach * narrowed.max(0.02))
    }));
    let columns = cuts.map(|(_, _, half)| {
        sprites::ribbon_lit_column(half / (light.glow.reach_share * half + reach_pixels))
    });
    let core = sections(std::array::from_fn(|index| {
        let (at, arc, half) = cuts[index];
        let tangent = body_sections[index].tangent;
        let up = Vec2::new(tangent.y, -tangent.x) * flip;
        let fade = smooth(0.0, light.glow.fade_pixels * pixel, arc);
        let width = half * fade * light.glow.core_reach;
        (at + up * (lean * half * fade), width)
    }));
    let body_tint = tinted(shine.tone, shine.strength);
    let core_tint = tinted(shine.core, shine.strength);
    for index in 0..RIBBON {
        let [near, far] = [columns[index], columns[index + 1]];
        let ends = [body_sections[index], body_sections[index + 1]];
        // The atlas columns increase along the quad. The halo is symmetric across it,
        // so reversing the quad samples the same profile when the ribbon narrows.
        let (ends, sprite) = if near <= far {
            (ends, sprites::ribbon_lit(near, far))
        } else {
            ([ends[1], ends[0]], sprites::ribbon_lit(far, near))
        };
        if let Some(draw) = strip_piece(camera, ends, body_tint, sprite) {
            builder.draw(band, draw);
        }
        let ends = [core[index], core[index + 1]];
        if let Some(draw) = strip_piece(camera, ends, core_tint, sprites::RIBBON_CORE) {
            builder.draw(band, draw);
        }
    }
}

/// A shared cross-section of two quads. The half-width is measured along the normal to
/// `tangent`, so either quad can reconstruct the same two points, even when reversed.
#[derive(Clone, Copy)]
struct Section {
    at: Vec2,
    half: f32,
    tangent: Vec2,
}

fn sections(samples: [(Vec2, f32); RIBBON + 1]) -> [Section; RIBBON + 1] {
    let segments: [(Vec2, f32); RIBBON] = std::array::from_fn(|index| {
        let delta = samples[index + 1].0 - samples[index].0;
        (delta.normalize_or(Vec2::X), delta.length())
    });
    std::array::from_fn(|index| {
        let (incoming, before) = segments[index.saturating_sub(1)];
        let (outgoing, after) = segments[index.min(RIBBON - 1)];
        let tangent = (incoming + outgoing).normalize_or(outgoing);
        let (at, half) = samples[index];
        // On a short, sharp bend the outer corner can pass the next cross-section.
        // Limit both ends to just under half the segment so its triangles cannot fold.
        let limit = |way: Vec2, length| 0.49 * length / way.perp_dot(tangent).abs().max(1e-5);
        Section {
            at,
            tangent,
            half: half
                .min(limit(incoming, before))
                .min(limit(outgoing, after)),
        }
    })
}

fn strip_piece(
    camera: &Camera,
    [from, to]: [Section; 2],
    tint: [u8; 4],
    sprite: Sprite,
) -> Option<Draw> {
    let delta = to.at - from.at;
    let long = delta.length();
    if long <= 0.0 {
        return None;
    }
    let way = delta / long;
    let across = |section: Section| section.half * way.dot(section.tangent).abs();
    let start = across(from);
    let end = across(to);
    let half = start.max(end);
    if half < 0.05 {
        return None;
    }
    let centre = from.at.lerp(to.at, 0.5);
    Some(
        Draw::glow(
            camera.screen_transform(centre, Vec2::new(long, 2.0 * half), way),
            tint,
            sprite,
            0.0,
        )
        .tapered(start / half, end / half)
        .mitred(slant(way, from.tangent), slant(way, to.tangent)),
    )
}

/// A point of a ribbon: where it is, how far along its curve, and its half-width there.
type Sample = (Vec2, f32, f32);

/// Where the `RIBBON + 1` cuts between the pieces of a ribbon along the curve `points` lie,
/// each with how far along the curve it is and the ribbon's half-width there (between those of
/// the fine points beside it), and how long the curve is; `None` for a curve of no length. The cuts are chosen among `FINE` points along the curve so that each piece takes an
/// equal share of the ribbon's length, of how much its width changes and of how far it turns,
/// weighed by `light.glow.width_weight` and `light.glow.turn_weight`: where an arrow flares near its head
/// or bends most, the pieces are short.
fn cut_ribbon(
    points: &[Vec2; 4],
    cut: Cut,
    lengths: Lengths<'_>,
) -> Option<([Sample; RIBBON + 1], f32)> {
    let light = lengths.light;
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
            + light.glow.width_weight * changed
            + light.glow.turn_weight * turned;
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

/// 0 at `from` and below, 1 at `to` and above, rising smoothly between, as GLSL's `smoothstep`.
fn smooth(from: f32, to: f32, t: f32) -> f32 {
    if to <= from {
        return if t >= to { 1.0 } else { 0.0 };
    }
    let t = ((t - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A ribbon's half-width where it has come `arc` of the `whole` points it is long, as the web
/// stage's `widthOf` has it.
fn width_of(cut: Cut, lengths: Lengths<'_>, arc: f32, whole: f32) -> f32 {
    let wide = lengths.wide;
    let s = if whole > 0.0 { arc / whole } else { 0.0 };
    let left = whole - arc;
    match cut {
        Cut::Swell => {
            let swell = lengths.light.swell;
            let bulge = (PI * s.powf(0.5f32.ln() / swell.peak.ln())).sin();
            let ends = (s / swell.start).clamp(0.0, 1.0)
                * ((1.0 - s) / swell.end).clamp(0.0, 1.0).powf(1.5);
            wide * (swell.least + (1.0 - swell.least) * bulge) * ends
        }
        Cut::Beam => {
            let beam_tuning = lengths.light.beam;
            let (head, neck) = (beam_tuning.head * wide, beam_tuning.neck * wide);
            let beam = (beam_tuning.from + (beam_tuning.to - beam_tuning.from) * s) * wide;
            if left < head {
                beam_tuning.dart * wide * left / head
            } else if left < neck {
                beam_tuning.dart * wide
                    + (beam - beam_tuning.dart * wide) * (left - head) / (neck - head)
            } else {
                beam
            }
        }
        Cut::Arrow => {
            let arrow = lengths.light.arrow;
            let scale = lengths.scale;
            let ramp = (s / arrow.ramp).clamp(0.0, 1.0);
            let smooth = ramp * ramp * (3.0 - 2.0 * ramp);
            let tail = arrow.tail + (1.0 - arrow.tail) * smooth.powf(arrow.rise);
            let flare = 1.0
                + arrow.flare
                    * (-((left - arrow.flare_at * scale) / (arrow.flare_spread * scale)).powi(2))
                        .exp();
            wide * tail
                * flare
                * (left / (arrow.fade * scale))
                    .clamp(0.0, 1.0)
                    .powf(arrow.fade_power)
        }
    }
}
