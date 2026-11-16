//! `fly`: the card travels from where it is to where it rests, turning over on the way if its face
//! changes. The flight's path depends on what the card is doing, as the web interface's `bowOf`
//! (`ui/src/stage/bows.ts`) and `lifts` (`ui/src/stage/lift.ts`) pick it:
//!
//! - A spell cast onto the stack bows off the straight line, sideways on the screen, toward its
//!   controller's edge of the table, the opponent's half as far and later, leaning the other way.
//! - A permanent spell resolving is thrown up off the stack and drops onto its place
//!   (`Shape::Thrown`).
//! - The viewer's land glides straight to its place. The opponent's drops out of their hand
//!   into the open table on the side away from where it is going, tilted, and swings back along
//!   the table to its place.
//! - Any other flight, such as a discard, goes along the straight line.
//!
//! Every flight but a land's also rises `verbs.fly.arc` millimetres toward the camera at the
//! middle of its time. The values are under `verbs.fly` in `data/beryl3d/space.json`.

use beryl_protocol::protocol::{Side, ZoneName};
use beryl_protocol::script::Act;
use glam::Vec3;

use super::{StepIn, StepOut};
use crate::data::space;
use crate::data::space::verbs::fly::{bows, thrown};
use crate::pose::{Bow, Pose, Segment, Shape};

pub const BUILT: bool = true;

/// One of the web interface's `BOWS`: how far off its straight way a flight goes, as a share of
/// that way's length, where it is farthest off (`peak` of its eased progress), how far it leans
/// there in degrees, and how far it also swings back across the table, as a share of how far
/// across the way runs.
struct Bowing {
    share: f32,
    peak: f32,
    lean: f32,
    back: f32,
}

/// A spell of the viewer's thrown onto the stack.
const CAST: Bowing = Bowing {
    share: bows::cast::SHARE,
    peak: bows::cast::PEAK,
    lean: bows::cast::LEAN,
    back: bows::cast::BACK,
};

/// A spell of the opponent's thrown onto the stack.
const THEIRS: Bowing = Bowing {
    share: bows::theirs::SHARE,
    peak: bows::theirs::PEAK,
    lean: bows::theirs::LEAN,
    back: bows::theirs::BACK,
};

/// A land the opponent plays.
const PLAYED: Bowing = Bowing {
    share: bows::played::SHARE,
    peak: bows::played::PEAK,
    lean: bows::played::LEAN,
    back: bows::played::BACK,
};

/// The path a flight takes.
enum Path {
    /// The straight line, rising `verbs.fly.arc` at its middle when `rises`.
    Straight {
        rises: bool,
    },
    /// Bowed toward one edge of the table: `toward` is 1 for the viewer's edge and -1 for the
    /// opponent's, as the web's `toward` is 1 down the stage.
    Bowed {
        bowing: &'static Bowing,
        toward: f32,
        rises: bool,
    },
    Thrown,
}

/// The path of the flight `act` makes (the web's `bowOf` and `lifts`).
fn path_of(act: &Act) -> Path {
    let Act::Fly { from, object, .. } = act else {
        return Path::Straight { rises: true };
    };
    let mine = object.controller == Side::You;
    let toward = if mine { 1.0 } else { -1.0 };
    match (*from, object.zone) {
        (_, ZoneName::Stack) => Path::Bowed {
            bowing: if mine { &CAST } else { &THEIRS },
            toward,
            rises: true,
        },
        (ZoneName::Stack, ZoneName::Battlefield) => Path::Thrown,
        (ZoneName::Hand, ZoneName::Battlefield) if mine => Path::Straight { rises: false },
        // The opponent's land bows toward the middle of the table, away from their own edge.
        (ZoneName::Hand, ZoneName::Battlefield) => Path::Bowed {
            bowing: &PLAYED,
            toward: -toward,
            rises: false,
        },
        _ => Path::Straight { rises: true },
    }
}

/// The bow of a flight from `from` to `to` (the web's `bowed`), seen by a camera whose right,
/// up and backward directions are `axes`. The web works on the screen, so this works in the
/// plane facing the camera: the way's `across` and `rise` are its lengths along the camera's
/// right and up, and the web's sideways offset `(-down, across)`, with its y running down the
/// screen, is `rise` along right and `-across` along up here. The web's clockwise lean is a
/// turn the other way about the direction toward the camera.
fn bow(bowing: &Bowing, toward: f32, (from, to): (Pose, Pose), axes: [Vec3; 3]) -> Bow {
    let [right, up, back] = axes;
    let way = to.position - from.position;
    let (across, rise) = (way.dot(right), way.dot(up));
    // Of the two sides of the line, the one toward that edge of the table.
    let side = if across * toward < 0.0 { -1.0 } else { 1.0 };
    let off = side * bowing.share;
    Bow {
        offset: right * (rise * off - across * bowing.back) - up * (across * off),
        sharpness: 0.5_f32.ln() / bowing.peak.ln(),
        lean: (side * bowing.lean).to_radians(),
        axis: back,
    }
}

pub fn play(step: &StepIn<'_>, out: &mut StepOut<'_>) {
    let (Some(subject), Some(from), Some(to)) = (step.subject, step.from, step.to) else {
        return;
    };
    let between = Segment::between(from, to, step.own, step.curve);
    let axes = step.places.camera().axes();
    let arc = |rises: bool| if rises { space::verbs::fly::ARC } else { 0.0 };
    let segment = match path_of(step.act) {
        Path::Straight { rises } => Segment {
            arc: arc(rises),
            ..between
        },
        Path::Bowed {
            bowing,
            toward,
            rises,
        } => Segment {
            arc: arc(rises),
            shape: Shape::Bowed(bow(bowing, toward, (from, to), axes)),
            ..between
        },
        Path::Thrown => Segment {
            shape: Shape::Thrown {
                // Up the screen by a share of the spell's width.
                rise: axes[1] * thrown::HEIGHT * space::card::WIDTH * from.scale,
                aloft: thrown::SCALE * from.scale,
            },
            ..between
        },
    };
    out.segment(subject, segment);
}
