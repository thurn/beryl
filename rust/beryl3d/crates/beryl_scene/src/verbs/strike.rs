//! `strike`: a creature lunges at what it deals combat damage to and comes back to its place,
//! as the web stage's `lunge` and `struck` (`ui/src/stage/tracks.ts`) and Arena's attackers do.
//! It rears back from what it strikes and rises toward the eye, slowing; hangs there until the
//! `poise` of its step's phases; dashes to stop just short of what it strikes at their `hit`,
//! coming down as it goes; stays on it until `verbs.strike.stay` of the step; and drifts back
//! onto its place, slowing, by the step's end. It leans toward the side what it strikes is on
//! from the start of its rear until it drifts back. What it strikes does not move.
//!
//! The web's blow lands over what it strikes, overlapping it by `STRIKE.bite` of the striker's
//! width, and on something much smaller it lands whole, shrinking (`STRIKE.small`). Here the
//! striker comes down to the table's height as it lands, where a card over the one it strikes
//! would pass through it, so it stops `near` outside that card's edge instead, and keeps its
//! size.
//!
//! The values are `verbs.strike` in `data/beryl3d/space.json`: `wind` and `stay` are shares of
//! the step, `rear` and `least` shares of the creature's card width, `lift` how much larger it
//! is drawn at its poise, `lean` an angle in degrees and `near` how far its middle stops outside
//! the edge of what it strikes, in millimetres.

use beryl_protocol::script::{Act, Step, Subject};
use glam::{Quat, Vec2, Vec3, Vec3Swizzles};

use super::{Interval, StepIn, StepOut};
use crate::data::space::verbs::strike;
use crate::pose::{Curve, Pose, Segment};
use crate::table::{CARD_HEIGHT, CARD_WIDTH};

pub const BUILT: bool = true;

/// The web stage's `out`, `1 - (1 - x)²`, which this cubic Bezier is exactly.
const OUT: Curve = Curve {
    x1: 1.0 / 3.0,
    y1: 2.0 / 3.0,
    x2: 2.0 / 3.0,
    y2: 1.0,
};

/// The web stage's `into`, `x²`, which this cubic Bezier is exactly.
const INTO: Curve = Curve {
    x1: 1.0 / 3.0,
    y1: 0.0,
    x2: 2.0 / 3.0,
    y2: 1.0 / 3.0,
};

/// The dash of the web stage's `struck`, `(x + x²) / 2`: half way between linear and `INTO`,
/// which this cubic Bezier is exactly.
const DASH: Curve = Curve {
    x1: 1.0 / 3.0,
    y1: 1.0 / 6.0,
    x2: 2.0 / 3.0,
    y2: 0.5,
};

pub fn play(step: &StepIn<'_>, out: &mut StepOut<'_>) {
    let (Some(subject), Some(from), Some(to)) = (step.subject, step.from, step.to) else {
        return;
    };
    let Act::Strike { to: struck, .. } = step.act else {
        return;
    };
    let Some(blow) = blow(step, from, *struck) else {
        out.instant();
        return;
    };
    if step.own.duration() == 0 {
        out.instant();
        return;
    }
    let (poise, hit) = phases(step);
    let at = |share: f32| {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss,
            reason = "a share from 0 to 1 of a step that lasts seconds"
        )]
        let offset = (f64::from(share.clamp(0.0, 1.0)) * step.own.duration() as f64) as u64;
        step.own.from + offset
    };
    let lean = Quat::from_rotation_z(blow.lean);
    let width = CARD_WIDTH * from.scale;
    let camera = step.places.camera();
    // Drawn `lift` larger: brought toward the eye by that share of its new distance.
    let toward_eye = (camera.eye - from.position).normalize_or_zero();
    let raised = camera.depth(from.position) * strike::LIFT / (1.0 + strike::LIFT);
    let poised = Pose {
        position: from.position - blow.unit * strike::REAR * width + toward_eye * raised,
        rotation: lean * from.rotation,
        ..from
    };
    let landed = Pose {
        position: from.position + blow.unit * blow.go,
        rotation: lean * from.rotation,
        ..from
    };
    let stay = strike::STAY.max(hit);
    let home = f32::midpoint(stay, 1.0);
    let half_way = between(landed, to, 0.5);
    let wind = strike::WIND.min(poise);
    let spans = [
        (from, poised, (0.0, wind), OUT),
        (poised, landed, (poise, hit), DASH),
        (landed, half_way, (stay, home), INTO),
        (half_way, to, (home, 1.0), OUT),
    ];
    // What the blow does shows as it lands, on the creature and on what it strikes.
    out.turn(subject, at(hit));
    out.turn(*struck, at(hit));
    for (start, end, (begins, ends), curve) in spans {
        let interval = Interval {
            from: at(begins),
            to: at(ends),
        };
        out.segment(subject, Segment::between(start, end, interval, curve));
    }
}

/// Where a strike's blow lands, from its creature's middle: along `unit`, `go` millimetres
/// away; and the angle, in radians about the table's up axis, it leans by.
struct Blow {
    unit: Vec3,
    go: f32,
    lean: f32,
}

/// The blow of the step's strike from `from` at `struck`, if what it strikes has a place.
fn blow(step: &StepIn<'_>, from: Pose, struck: Subject) -> Option<Blow> {
    let (at, half) = match struck {
        Subject::Object { .. } => {
            let pose = step.places.pose(struck, step.own.from)?;
            (pose.position.xy(), extent(pose))
        }
        // A player's anchor stands before the eye: what the strike goes at is the point of the
        // table under it on the screen. It stops with its own edge at the anchor, not over it.
        Subject::Player { .. } => {
            let anchor = step.places.pose(struck, step.own.from)?.position;
            (
                under(step.places.camera().eye, anchor, from.position.z),
                extent(from),
            )
        }
        Subject::Ability { .. } => return None,
    };
    let line = at - from.position.xy();
    let far = line.length();
    let unit = line.normalize_or(Vec2::Y);
    // How far from what it strikes its middle stops: `near` outside that thing's edge, or for a
    // player, with its own edge `near` short of the anchor.
    let edge = [unit.x, unit.y]
        .into_iter()
        .zip([half.x, half.y])
        .filter(|(along, _)| *along != 0.0)
        .map(|(along, half)| half / along.abs())
        .fold(f32::INFINITY, f32::min);
    let short = edge + strike::NEAR;
    // Two creatures that strike each other meet half way.
    let share = if meets_another(step) { 0.5 } else { 1.0 };
    let least = strike::LEAST * CARD_WIDTH * from.scale;
    let go = ((far - short) * share).max(least);
    let camera = step.places.camera();
    let leftward =
        camera.to_screen(from.position + unit.extend(0.0)).x < camera.to_screen(from.position).x;
    // It leans clockwise on the screen, a negative turn about the up axis, unless what it
    // strikes is to its left (`lunge` in tracks.ts leans the same way).
    let lean = strike::LEAN.to_radians() * if leftward { 1.0 } else { -1.0 };
    Some(Blow {
        unit: unit.extend(0.0),
        go,
        lean,
    })
}

/// Half the width and depth a card at `pose` covers on the table, turned as it lies.
fn extent(pose: Pose) -> Vec2 {
    let across = pose.rotation * Vec3::new(CARD_WIDTH / 2.0, 0.0, 0.0);
    let along = pose.rotation * Vec3::new(0.0, CARD_HEIGHT / 2.0, 0.0);
    Vec2::new(
        across.x.abs() + along.x.abs(),
        across.y.abs() + along.y.abs(),
    ) * pose.scale
}

/// The point at height `z` on the line from `eye` through `point`: what lies under `point` on
/// the screen.
fn under(eye: Vec3, point: Vec3, z: f32) -> Vec2 {
    let drop = eye.z - point.z;
    if drop <= 0.0 {
        return point.xy();
    }
    (eye + (point - eye) * ((eye.z - z) / drop)).xy()
}

/// Whether the step's strike and the one beside it in its beat are the two blows of one fight:
/// each creature strikes the other (`meets` in `tracks.ts`). The session puts the blow dealt
/// back right after the strike it answers.
fn meets_another(step: &StepIn<'_>) -> bool {
    let next = step.beat.get(step.index + 1);
    let previous = step
        .index
        .checked_sub(1)
        .and_then(|index| step.beat.get(index));
    [next, previous]
        .into_iter()
        .flatten()
        .any(|other| meets(step.act, other))
}

fn meets(act: &Act, other: &Step) -> bool {
    matches!(
        (act, &other.act),
        (
            Act::Strike { source: a, to: Subject::Object { slot: a_to }, .. },
            Act::Strike { source: b, to: Subject::Object { slot: b_to }, .. },
        ) if a_to == b && b_to == a
    )
}

/// The `poise` and `hit` of the step's phases, as shares of the step. A step that names none
/// dashes and lands at once, as the web stage's `strikePhases` has it.
fn phases(step: &StepIn<'_>) -> (f32, f32) {
    let named = |name: &str| {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a share of a step from 0 to 1"
        )]
        step.timing
            .phases
            .iter()
            .find(|phase| phase.name == name)
            .map_or(0.0, |phase| phase.at as f32)
    };
    let poise = named("poise");
    (poise, named("hit").max(poise))
}

/// The pose `share` of the way from `a` to `b`.
fn between(a: Pose, b: Pose, share: f32) -> Pose {
    Pose {
        position: a.position.lerp(b.position, share),
        rotation: a.rotation.slerp(b.rotation, share),
        scale: a.scale + (b.scale - a.scale) * share,
        opacity: a.opacity + (b.opacity - a.opacity) * share,
        turn: a.turn + (b.turn - a.turn) * share,
    }
}
