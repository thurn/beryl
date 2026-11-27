//! Procedural Enhanced-tier luminous line ribbons and faceted spearheads (`Vfx::LineRibbon` and
//! `Vfx::LineHead`), emitted in screen-space overlay coordinates from a cubic Bezier curve.

use glam::Vec2;
use kit_frame::frame::{Band, Draw, FrameBuilder, LineStyle, vfx};
use kit_space::camera::Camera;

/// How many tapered, mitred ribbon quads make up one procedural line.
pub const RIBBON: usize = 20;

/// Fine curve samples used to distribute ribbon cuts by arc length and curvature.
const FINE: usize = 32;

/// Inputs for emitting one procedural Enhanced-tier luminous line along a cubic Bezier curve.
#[derive(Clone, Copy, Debug)]
pub struct LineDraw {
    pub points: [Vec2; 4],
    pub style: LineStyle,
    pub unit_scale: f32,
    pub opacity: f32,
    pub time_s: f32,
    pub turn_weight: f32,
    pub seed_id: u32,
    pub band: Band,
}

/// Reference `(ribbon_half_width, head_width)` in logical desktop points for each Enhanced line
/// style.
const fn dimensions(style: LineStyle) -> (f32, f32) {
    match style {
        LineStyle::Aim | LineStyle::FriendlyTarget | LineStyle::HostileTarget => (25.0, 23.0),
        LineStyle::Block => (16.5, 15.0),
        LineStyle::Attack => (15.5, 14.0),
        LineStyle::Source => (13.0, 11.0),
    }
}

/// Width multiplier along normalized arc length `s` in `[0, 1]`.
fn taper(style: LineStyle, s: f32) -> f32 {
    let s = s.clamp(0.0, 1.0);
    match style {
        LineStyle::Aim
        | LineStyle::FriendlyTarget
        | LineStyle::HostileTarget
        | LineStyle::Block => {
            if s <= 0.5 {
                1.0
            } else {
                1.0 - 0.45 * ((s - 0.5) * 2.0)
            }
        }
        LineStyle::Attack => 0.72 + 0.28 * s,
        LineStyle::Source => 1.0,
    }
}

/// Writes an Enhanced-tier procedural line ribbon (`RIBBON` tapered, mitred `Vfx::LineRibbon`
/// segments) plus one faceted glowing spearhead (`Vfx::LineHead`) at the tip.
pub fn write(camera: &Camera, line: &LineDraw, builder: &mut FrameBuilder<'_>) {
    if line.opacity <= 0.0 {
        return;
    }
    let Some((cuts, whole)) = cut(&line.points, line.style, line.turn_weight) else {
        return;
    };
    let (base_half, base_head_w) = dimensions(line.style);
    let ribbon_half = base_half * line.unit_scale;
    // Shrink ribbon and head proportionally on very short curves so the spearhead never overwhelms
    // tight arches.
    let raw_head_len = base_head_w * line.unit_scale * vfx::LINE_HEAD_ASPECT;
    let shrink = (whole / (raw_head_len * 2.8).max(1.0)).clamp(0.42, 1.0);
    let head_w = base_head_w * line.unit_scale * shrink;
    let head_len = head_w * vfx::LINE_HEAD_ASPECT;
    let half_scale = ribbon_half * shrink.sqrt();

    let ribbon_sections = sections(cuts.map(|(at, _, mult)| (at, half_scale * mult)));
    let len_ratio = (whole / half_scale.max(1.0)).max(1.0);

    for index in 0..RIBBON {
        let from = ribbon_sections[index];
        let to = ribbon_sections[index + 1];
        let delta = to.at - from.at;
        let long = delta.length();
        if long <= 0.0 {
            continue;
        }
        let way = delta / long;
        let across = |section: Section| section.half * way.dot(section.tangent).abs();
        let start = across(from);
        let end = across(to);
        let half = start.max(end);
        if half < 0.05 {
            continue;
        }
        let centre = from.at.lerp(to.at, 0.5);
        let v_start = cuts[index].1 / whole;
        let v_end = cuts[index + 1].1 / whole;
        builder.draw(
            line.band,
            Draw::vfx_line_ribbon(
                camera.screen_transform(centre, Vec2::new(long, 2.0 * half), way),
                line.opacity,
                line.style,
                [start / half, end / half],
                [slant(way, from.tangent), slant(way, to.tangent)],
                [line.time_s, v_start, v_end, len_ratio],
            ),
        );
    }

    // Seat the faceted spearhead right at the curve endpoint along the last ribbon segment so the
    // head never diverges from the approaching line on tight curves.
    let tip = line.points[3];
    let tangent = (tip - cuts[RIBBON - 1].0)
        .try_normalize()
        .or_else(|| (line.points[3] - line.points[2]).try_normalize())
        .or_else(|| (tip - line.points[0]).try_normalize());
    if let Some(tip_dir) = tangent {
        let head_centre = tip - tip_dir * (0.5 * head_len);
        let quad_size = Vec2::new(
            head_len * (1.0 + vfx::LINE_HEAD_PAD[0]),
            head_w * (1.0 + vfx::LINE_HEAD_PAD[1]),
        );
        #[expect(
            clippy::cast_precision_loss,
            reason = "small hashed seed value fits f32 precision"
        )]
        let seed = ((line.seed_id.wrapping_mul(2_654_435_761)) >> 16) as f32 / 65_536.0;
        builder.draw(
            line.band,
            Draw::vfx_line_head(
                camera.screen_transform(head_centre, quad_size, tip_dir),
                line.opacity,
                line.style,
                [line.time_s, 1.0, seed, vfx::LINE_HEAD_ASPECT],
            ),
        );
    }
}

/// Evaluates the cubic Bezier curve `points` at `t` in `[0, 1]`.
fn along(points: &[Vec2; 4], t: f32) -> Vec2 {
    let u = 1.0 - t;
    points[0] * (u * u * u)
        + points[1] * (3.0 * u * u * t)
        + points[2] * (3.0 * u * t * t)
        + points[3] * (t * t * t)
}

/// A shared cross-section of two adjacent ribbon quads. The half-width is measured along the
/// normal to `tangent` so adjacent quads meet without seams or gaps.
#[derive(Clone, Copy, Debug)]
pub struct Section {
    pub at: Vec2,
    pub half: f32,
    pub tangent: Vec2,
}

/// Computes bisector tangents and fold-safe half-widths for each ribbon cut.
#[must_use]
pub fn sections(samples: [(Vec2, f32); RIBBON + 1]) -> [Section; RIBBON + 1] {
    let segments: [(Vec2, f32); RIBBON] = std::array::from_fn(|index| {
        let delta = samples[index + 1].0 - samples[index].0;
        (delta.normalize_or(Vec2::X), delta.length())
    });
    std::array::from_fn(|index| {
        let (incoming, before) = segments[index.saturating_sub(1)];
        let (outgoing, after) = segments[index.min(RIBBON - 1)];
        let tangent = (incoming + outgoing).normalize_or(outgoing);
        let (at, half) = samples[index];
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

/// Mitre slant along a segment direction `along` to meet a cross-section normal to `halves`.
#[must_use]
pub fn slant(along: Vec2, halves: Vec2) -> f32 {
    let across = Vec2::new(along.y, -along.x);
    let facing = along.dot(halves);
    if facing.abs() < 1e-3 {
        return 0.0;
    }
    -across.dot(halves) / facing
}

type Sample = (Vec2, f32, f32);

/// Samples `RIBBON + 1` cuts along `points` weighted by arc length and bend curvature, storing
/// each point's Enhanced width multiplier in `Sample.2`.
fn cut(
    points: &[Vec2; 4],
    style: LineStyle,
    turn_weight: f32,
) -> Option<([Sample; RIBBON + 1], f32)> {
    let mut fine: [Sample; FINE + 1] = [(Vec2::ZERO, 0.0, 0.0); FINE + 1];
    let mut arc = 0.0;
    for number in 0..=FINE {
        #[expect(clippy::cast_precision_loss, reason = "a small count")]
        let t = number as f32 / FINE as f32;
        let at = along(points, t);
        if number > 0 {
            arc += fine[number - 1].0.distance(at);
        }
        fine[number] = (at, arc, 0.0);
    }
    let whole = arc;
    if whole <= 0.0 {
        return None;
    }
    for point in &mut fine {
        point.2 = taper(style, point.1 / whole);
    }
    let mut measure = [0.0_f32; FINE + 1];
    for number in 1..=FINE {
        let (before, here) = (fine[number - 1], fine[number]);
        let turned = if number >= 2 {
            let earlier = (before.0 - fine[number - 2].0).normalize_or_zero();
            let later = (here.0 - before.0).normalize_or_zero();
            earlier.perp_dot(later).abs()
        } else {
            0.0
        };
        measure[number] = measure[number - 1] + (here.1 - before.1) / whole + turn_weight * turned;
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
