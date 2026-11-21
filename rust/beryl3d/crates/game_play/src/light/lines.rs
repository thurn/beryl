//! The shape of an arrow or line on the screen (UI Conventions, Verbs, `point`): where it leaves
//! one thing and meets the other, given only how the two lie on the screen in one frame, as the
//! web stage's `wayOf` (`ui/src/stage/lines.ts`) works it out. A shape is a function of its two
//! ends alone, so a line is never anywhere its ends are not, and it is the same however its frame
//! was reached. Nothing here routes round other cards: Arena's arcs go over whatever lies
//! between.
//!
//! What each kind says is drawn its own way: an arrow at a target is a bowed arc from the spell
//! or ability over the top to what it is aimed at, coming onto a card from the open table in
//! front of it; the line from an ability to its source hangs low over the table between the two
//! (`hung`); a line of combat is straight, from the middle of its creature to the middle of what
//! it points at.
//!
//! Every point is in logical points from the window's top left corner, y down, and the values of
//! `light` in `data/beryl3d/space-constants.json` are copied from `lines.ts`: shares of a way or of an
//! end's half-height, which do not depend on units, but for `hang.graze`, in points, which the
//! web's desktop stage measures as pixels.

use beryl_protocol::script::Arrow;
use glam::Vec2;

use kit_base::data::space::light::{bow, hang, onto};

/// One end of a line as it lies on the screen: its middle, how far it reaches from there across
/// and down, as a box or, `round`, as the ellipse inside that box (a player's life total, as the
/// web's `reachOfLife` has it), and which way its controller faces across the table, up the
/// screen (-1) or down it (1); 0 for what faces neither way. An end that reaches nowhere is a
/// point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct End {
    pub middle: Vec2,
    pub half: Vec2,
    pub round: bool,
    pub toward: f32,
}

/// The whole way of the line that says `arrow` from `from` to `to`, as a quadratic curve from its
/// first point round its second to its third (a line of combat is straight, its second point half
/// way); `None` where there is nothing to draw: an ability lies over the source it lifts out of,
/// or a creature over what it strikes. The line to a source sags to the height `sag` on the
/// screen, where the layout gives one.
#[must_use]
pub fn way_of(arrow: Arrow, from: End, to: End, sag: Option<f32>) -> Option<[Vec2; 3]> {
    match arrow {
        Arrow::Attack | Arrow::Block => {
            let [a, b] = straight(from, to)?;
            Some([a, a.lerp(b, 0.5), b])
        }
        Arrow::Source => {
            if overlap(from, to) {
                return None;
            }
            match sag {
                Some(sag) => Some(hung(from, to, sag)),
                None => arc(from, to),
            }
        }
        Arrow::FriendlyTarget | Arrow::HostileTarget => {
            // Onto a card from the table in front of it; round the side of one that stands right
            // by; over the top to a player; and into the strip of the stack item under its own.
            // But for that strip, the head lies on the face of what it is aimed at.
            let apart = if overlap(from, to) {
                None
            } else {
                arc(from, to)
            };
            let faced = apart
                .filter(|_| to.toward != 0.0 && !tight(from, to))
                .and_then(|_| onto_card(from, to));
            Some(match faced.or(apart) {
                Some([a, c, b]) => [a, c, b.lerp(to.middle, onto::INTO)],
                None => stack_loop(from, to),
            })
        }
    }
}

/// The cubic Bezier curve that is exactly the quadratic `way`: its start, two control points and
/// its end.
#[must_use]
pub fn cubic([a, c, b]: [Vec2; 3]) -> [Vec2; 4] {
    [a, a + (c - a) * (2.0 / 3.0), b + (c - b) * (2.0 / 3.0), b]
}

/// The point `t` of the way along the cubic curve `points`.
#[must_use]
pub fn along(points: &[Vec2; 4], t: f32) -> Vec2 {
    let u = 1.0 - t;
    points[0] * (u * u * u)
        + points[1] * (3.0 * u * u * t)
        + points[2] * (3.0 * u * t * t)
        + points[3] * (t * t * t)
}

/// Where the way from `end`'s middle toward `point` crosses its edge, its box's or its
/// ellipse's; `None` if `point` lies within it. A point's edge is the point itself.
fn edge(end: End, point: Vec2) -> Option<Vec2> {
    let way = point - end.middle;
    if end.half.x <= 0.0 || end.half.y <= 0.0 {
        return (way != Vec2::ZERO).then_some(end.middle);
    }
    let share = way / end.half;
    let out = if end.round {
        share.length()
    } else {
        share.x.abs().max(share.y.abs())
    };
    (out > 1.0).then(|| end.middle + way / out)
}

/// Whether the two ends lie over one another: the middle of either is within the other.
fn overlap(a: End, b: End) -> bool {
    edge(a, b.middle).is_none() || edge(b, a.middle).is_none()
}

/// The smaller half-height of two ends, the unit the arcs bow by.
fn unit(a: End, b: End) -> f32 {
    a.half.y.min(b.half.y)
}

/// Whether two ends that lie apart have little between them, along the straight way from one's
/// middle to the other's.
fn tight(from: End, to: End) -> bool {
    match (edge(from, to.middle), edge(to, from.middle)) {
        (Some(near), Some(there)) => there.distance(near) < bow::CLOSE * unit(from, to),
        _ => true,
    }
}

/// The arc of an arrow onto a card that faces across the table: out of `from` and round a point
/// in front of `to`, on the open table, so that it comes onto its target square, from the side no
/// neighbour of its row stands on (Arena's comes down onto the middle of its target,
/// `I3Te0m1bBFw 31:35.08`). `None` where `from` itself stands in front of its target.
fn onto_card(from: End, to: End) -> Option<[Vec2; 3]> {
    let back = from.middle - to.middle;
    let unit = unit(from, to);
    let ahead = (onto::SHARE * back.length())
        .max(onto::LEAST * unit)
        .min(onto::MOST * unit);
    let lean = (onto::LEAN * back.x / to.half.x).clamp(-1.0, 1.0);
    let front = Vec2::new(
        to.middle.x + lean * onto::ASIDE * to.half.x,
        to.middle.y + to.toward * (to.half.y + ahead),
    );
    Some([edge(from, front)?, front, edge(to, front)?])
}

/// The arc between two ends that lie apart: out of `from` and into `to` round a point off the
/// straight way between their middles, on the side that is up the screen, so that it goes over
/// what lies between (Arena's "over the top"). Two ends with little between them leave no room
/// for an arrow there, so its arc goes round the side of both instead.
fn arc(from: End, to: End) -> Option<[Vec2; 3]> {
    let near = edge(from, to.middle)?;
    let there = edge(to, from.middle)?;
    let way = to.middle - from.middle;
    let far = way.length();
    if far <= 0.0 {
        return None;
    }
    let unit = unit(from, to);
    let side = if way.x >= 0.0 { 1.0 } else { -1.0 };
    let up = Vec2::new(side * way.y, -side * way.x) / far;
    // How far each end reaches the way the arc bows.
    let out = |end: End| up.x.abs() * end.half.x + up.y.abs() * end.half.y;
    let bowed = if tight(from, to) {
        out(from).max(out(to)) + bow::ROUND * unit
    } else {
        (bow::SHARE * far)
            .max(bow::LEAST * unit)
            .min(bow::MOST * unit)
    };
    let over = from.middle.lerp(to.middle, 0.5) + up * bowed;
    // Out of each end toward that point, or straight toward the other end where the point is
    // within one of them.
    Some([
        edge(from, over).unwrap_or(near),
        over,
        edge(to, over).unwrap_or(there),
    ])
}

/// The arc from a stack item to the one it lies over (UI Conventions, "Two items on the stack").
/// The one on top lies a little across and down from the other, of which a strip shows along one
/// edge. The arc leaves the side of `from` that faces the same way, swings out clear of both and
/// comes back into the end of that strip, so that its head lies on the table beside the stack.
fn stack_loop(from: End, to: End) -> [Vec2; 3] {
    let side = if from.middle.x >= to.middle.x {
        -1.0
    } else {
        1.0
    };
    let high = if from.middle.y >= to.middle.y {
        -1.0
    } else {
        1.0
    };
    let shown = to.middle.y + high * to.half.y;
    let covered = from.middle.y + high * from.half.y;
    let strip = match (covered - shown).abs() {
        0.0 => 0.24 * to.half.y,
        strip => strip,
    };
    let a = Vec2::new(
        from.middle.x + side * from.half.x,
        from.middle.y + high * from.half.y * 0.5,
    );
    let b = Vec2::new(to.middle.x + side * to.half.x, shown - high * strip / 2.0);
    let out = 0.7 * from.half.x.min(to.half.x);
    let clear = if side < 0.0 {
        a.x.min(b.x)
    } else {
        a.x.max(b.x)
    };
    [a, Vec2::new(clear + side * out, f32::midpoint(a.y, b.y)), b]
}

/// The line from an ability `from` to its `source`, sagging at most to the screen height `sag`.
/// Arena's leaves the bottom of its source going down, hangs low over the table and comes up into
/// the bottom of the stack card, from either seat (`0VsYGs88NXA 19:39.85`, `2VW6ZzmS8U4 32:16.0`).
/// Its lowest point lies below the lower of its two ends by `hang::DEEP` of how far apart they
/// are across (half a card at least), and no lower than `sag`. A source that lies under the
/// stack card has a short curve from the bottom of the stack card down into the top of the
/// source; one above it whose hanging curve would cross the stack card has a short curve from the
/// top of the stack card up into the bottom of the source.
fn hung(from: End, source: End, sag: f32) -> [Vec2; 3] {
    let edge = |end: End, down: f32| Vec2::new(end.middle.x, end.middle.y + down * end.half.y);
    let a = edge(from, 1.0);
    let top = edge(source, -1.0);
    let b = edge(source, 1.0);
    let across = (a.x - top.x).abs();
    // A short curve that bows `side` of the way between its ends.
    let short = |start: Vec2, end: Vec2, side: f32| {
        let round = Vec2::new(
            f32::midpoint(start.x, end.x) + side * hang::BOW * (end.y - start.y).abs(),
            f32::midpoint(start.y, end.y),
        );
        [start, round, end]
    };
    if top.y >= a.y && across <= from.half.x + source.half.x {
        return short(a, top, 1.0);
    }
    let deepest = sag.min(a.y.max(b.y) + (source.half.y / 2.0).max(hang::DEEP * across));
    // A curve from `a` round `c` to `b` is lowest at (a.y b.y - c.y²) / (a.y + b.y - 2 c.y),
    // which is `deepest` where `c` lies this low.
    let low = deepest + ((deepest - a.y) * (deepest - b.y)).max(0.0).sqrt();
    let hanging = [a, Vec2::new(b.x + hang::ALONG * (a.x - b.x), low), b];
    let crown = edge(from, -1.0);
    if b.y <= crown.y && crosses(hanging, from) {
        short(crown, b, if b.x < crown.x { -1.0 } else { 1.0 })
    } else {
        hanging
    }
}

/// How many points along a curve `crosses` samples.
const CROSS_STEPS: u16 = 64;

/// Whether the quadratic curve from `a` round `c` to `b` passes over `end` anywhere between its
/// own two ends, further in than `hang::GRAZE` points, less than the line is wide: a curve that
/// clips a corner of the stack card by less than that still hangs.
fn crosses([a, c, b]: [Vec2; 3], end: End) -> bool {
    let inside = end.half - Vec2::splat(hang::GRAZE);
    (1..CROSS_STEPS).any(|i| {
        let t = f32::from(i) / f32::from(CROSS_STEPS);
        let p = a.lerp(c, t).lerp(c.lerp(b, t), t);
        (p.x - end.middle.x).abs() < inside.x && (p.y - end.middle.y).abs() < inside.y
    })
}

/// A line of combat's two ends: from the middle of the creature it leaves to the middle of what
/// it points at, as Arena draws a block line (`IEXcjBn3BXk 22:23.0`) and an attack line to the
/// planeswalker attacked (`UrGalw_f35w 48:56.6`). `None` where the two lie over each other.
fn straight(from: End, to: End) -> Option<[Vec2; 2]> {
    let way = (to.middle - from.middle).abs();
    let apart = way.x >= from.half.x / 2.0 || way.y >= from.half.y / 2.0;
    apart.then_some([from.middle, to.middle])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ends of `combat-attackers` at 102500 ms in a 1600x900 window: an ability on the stack
    /// and its tapped source on the opponent's row, up and to the left of it. Its hanging curve
    /// clips the stack card's bottom left corner by less than a point, and still hangs low rather
    /// than rising from the top of the stack card.
    #[test]
    fn a_curve_that_grazes_the_stack_card_still_hangs() {
        let ability = End {
            middle: Vec2::new(1342.33, 400.0),
            half: Vec2::new(92.79, 129.61),
            round: false,
            toward: 1.0,
        };
        let source = End {
            middle: Vec2::new(1200.62, 159.47),
            half: Vec2::new(51.14, 34.37),
            round: false,
            toward: 1.0,
        };
        let [a, c, b] = way_of(Arrow::Source, ability, source, Some(810.0)).unwrap();
        assert!(
            a.distance(Vec2::new(1342.33, 529.61)) < 0.01,
            "leaves at {a}"
        );
        assert!(c.y > a.y, "the curve hangs below the stack card, at {c}");
        assert!(
            b.distance(Vec2::new(1200.62, 193.84)) < 0.01,
            "meets at {b}"
        );
    }
}
