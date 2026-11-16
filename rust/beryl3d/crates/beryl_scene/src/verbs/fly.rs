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
//! - A card the viewer draws alone flies from the library to a place beside the stack, bowing
//!   up the screen, is held up there large to be read, sinking a little toward the hand, and
//!   drops into the hand (the web's `heldUp` in `ui/src/stage/drawn.ts`). Each of several drawn
//!   together sweeps past that place without stopping (`swept`).
//! - The opponent's draw drops toward the middle of the table and rises into their hand, and so
//!   does a card no one can read that an effect puts in a hand from a library, farther.
//! - An opponent's discard drops along the line from hand to graveyard. Other flights also
//!   follow a straight line across the table.
//!
//! Flights that rise use `verbs.fly.arc` for their height at the middle of the step. A card
//! whose face changes turns over top over bottom
//! at moments of its own (`Turning`): a drawn card as it leaves the library, any other a little
//! before half way. The values are under `verbs.fly` in `data/beryl3d/space.json`.
//!
//! The web turns a card that lands face down the other way up the table (the opponent's draw)
//! half round as it leaves, so that it never spins in the table's plane on the way. Beryl 3D's
//! opponent's hand faces the camera the same way up as the library, so the flight has no half
//! turn to take out.

use beryl_protocol::protocol::{Side, Why, ZoneName};
use beryl_protocol::script::Act;
use glam::{Quat, Vec3};

use super::{Interval, StepIn, StepOut};
use crate::data::space;
use crate::data::space::verbs::fly::{bows, held, thrown, turn};
use crate::pose::{Bow, Curve, Pose, Segment, Shape, Turning};
use crate::table::Camera;
use beryl_protocol::script::Subject;

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

/// A card drawn that no one reads, the opponent's.
const DRAWN: Bowing = Bowing {
    share: bows::drawn::SHARE,
    peak: bows::drawn::PEAK,
    lean: bows::drawn::LEAN,
    back: bows::drawn::BACK,
};

/// A card no one can read that an effect takes from a library into a hand. It drops far
/// into the table, with a shallow sideways swing so it stays inside the window on leaving
/// the pile.
const TAKEN: Bowing = Bowing {
    share: bows::taken::SHARE,
    peak: bows::taken::PEAK,
    lean: bows::taken::LEAN,
    back: bows::taken::BACK,
};

/// A card the viewer draws alone, on its way to where it is held up.
const HELD_IN: Bowing = Bowing {
    share: held::bow::SHARE,
    peak: held::bow::PEAK,
    lean: held::bow::LEAN,
    back: held::bow::BACK,
};

/// A card that turns over on a flight that is not a draw: edge-on a little before half way.
const CAST_TURN: Turning = Turning {
    at: turn::cast::AT,
    each: turn::cast::EACH,
};

/// A drawn card's turn: as it leaves the library.
const DRAW_TURN: Turning = Turning {
    at: turn::draw::AT,
    each: turn::draw::EACH,
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
    /// The viewer's card drawn alone: held up beside the stack, then dropped into the hand.
    Held,
    /// One of several the viewer draws together: past where one alone is held up.
    Swept,
}

/// Whether `act` is a card drawn, or a card flown as one is: the flight from a library to a
/// hand that the game calls a draw, or one no one at the table can read that an effect puts
/// there (the web's `draws` in `ui/src/stage/tracks.ts`).
fn draws(act: &Act) -> bool {
    matches!(act, Act::Fly { from: ZoneName::Library, why, object, .. }
        if object.zone == ZoneName::Hand && (*why == Why::Draw || object.name.is_none()))
}

/// The path of the flight `act` makes, whose case is `case` (the web's `bowOf` and `lifts`,
/// and its `heldUp` and `swept` for the cases `draw` and `draw of several`).
fn path_of(act: &Act, case: Option<&str>) -> Path {
    let Act::Fly {
        from, object, why, ..
    } = act
    else {
        return Path::Straight { rises: true };
    };
    let mine = object.controller == Side::You;
    let toward = if mine { 1.0 } else { -1.0 };
    match case {
        Some("draw") => return Path::Held,
        Some("draw of several") => return Path::Swept,
        _ => {}
    }
    if draws(act) {
        // Toward the middle of the table, away from its player's edge.
        return Path::Bowed {
            bowing: if *why == Why::Draw { &DRAWN } else { &TAKEN },
            toward: -toward,
            rises: true,
        };
    }
    match (*from, object.zone) {
        // A spell cast from a hand goes between two places held near the camera, and never
        // leaves the table to rise above it: a rise along the table's height would carry it up
        // the screen against its bow.
        (_, ZoneName::Stack) => Path::Bowed {
            bowing: if mine { &CAST } else { &THEIRS },
            toward,
            rises: *from != ZoneName::Hand,
        },
        (ZoneName::Stack, ZoneName::Battlefield) => Path::Thrown,
        // The opponent's hand lies at the top of the frame. A discard drops from the
        // hand to the graveyard, so it should not rise farther above the table first.
        (ZoneName::Hand, ZoneName::Graveyard) if !mine && *why == Why::Discard => {
            Path::Straight { rises: false }
        }
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

/// The bow of a flight from `from` to `to` (the web's `bowed`), seen by `camera`. The web works
/// on the screen, so this does too: the way's `across` and `rise` are how far its far end is
/// to the right of and above its near end on the window, in millimetres at the depth of the
/// way's middle, and the offset lies in the plane facing the camera there. Measuring them along
/// the camera's axes instead reads the way wrongly where its ends lie at very different depths:
/// a hand near the eye can be higher on the window than a library far off and yet lower along
/// the camera's up. The web's sideways offset `(-down, across)`, with its y running down the
/// screen, is `rise` along right and `-across` along up here. The web's clockwise lean is a
/// turn the other way about the direction toward the camera.
fn bow(bowing: &Bowing, toward: f32, (from, to): (Pose, Pose), camera: &Camera) -> Bow {
    let [right, up, back] = camera.axes();
    let middle = from.position.midpoint(to.position);
    let points = camera.to_screen(middle + right).x - camera.to_screen(middle).x;
    let way = (camera.to_screen(to.position) - camera.to_screen(from.position)) / points;
    let (across, rise) = (way.x, -way.y);
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

/// Where the viewer's drawn card is held up to be read, for a card that rests at `rest` in
/// the hand: upright beside the stack, `held.size` times as tall as a card of the hand, placed
/// relative to the camera as the hand is (the web's `beside` in `ui/src/stage/desktop.ts`).
fn held_place(step: &StepIn<'_>, rest: Pose) -> Pose {
    let height = step.places.layout().hand.height * held::SIZE;
    let ndc = held::NDC;
    let pose = step
        .places
        .camera()
        .screen_pose(ndc, held::DEPTH, Quat::IDENTITY, height);
    Pose {
        turn: rest.turn,
        opacity: rest.opacity,
        ..pose
    }
}

/// Whether a card's face changes on its way from `from` to `to`.
fn turns(from: Pose, to: Pose) -> bool {
    (from.turn - to.turn).abs() > 1e-4
}

/// The pose `u` of the way from `from` to `to`, each part of it alike.
fn lerp(from: Pose, to: Pose, u: f32) -> Pose {
    Pose {
        position: from.position.lerp(to.position, u),
        rotation: from.rotation.slerp(to.rotation, u),
        scale: from.scale + (to.scale - from.scale) * u,
        opacity: from.opacity + (to.opacity - from.opacity) * u,
        turn: from.turn + (to.turn - from.turn) * u,
    }
}

/// The part of the step's time from `from` to `to`, as shares of it.
fn part(own: Interval, from: f32, to: f32) -> Interval {
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a step lasts seconds, far below f64's exact range"
    )]
    let at = |share: f32| own.from + (own.duration() as f64 * f64::from(share)) as u64;
    Interval {
        from: at(from),
        to: at(to),
    }
}

/// The viewer's card drawn alone, as the web's `heldUp` has it, in three segments over the
/// step: the flight in to where it is held, `held.in` of the step, bowed up the screen and
/// turning over as it leaves the library; the hold, `held.hold` of the step, sinking evenly by
/// `held_sink` of the way toward the hand; and the drop into the hand, leaving that pace and
/// quickening, with no jolt between the two.
fn held_up(step: &StepIn<'_>, (from, to): (Pose, Pose), out: &mut StepOut<'_>, subject: Subject) {
    let up = held_place(step, to);
    let lowered = lerp(up, to, space::verbs::fly::HELD_SINK);
    let (flying, holding) = (held::IN, held::HOLD);
    let turning = turns(from, to).then_some(Turning {
        at: DRAW_TURN.at / flying,
        each: DRAW_TURN.each / flying,
    });
    out.segment(
        subject,
        Segment {
            shape: Shape::Bowed(bow(&HELD_IN, -1.0, (from, up), step.places.camera())),
            turning,
            ..Segment::between(from, up, part(step.own, 0.0, flying), step.curve)
        },
    );
    out.segment(
        subject,
        Segment::between(
            up,
            lowered,
            part(step.own, flying, flying + holding),
            Curve::LINEAR,
        ),
    );
    // The drop goes `pace x + (1 - pace) x²` of its way in `x` of its time, so that it leaves
    // at the pace the hold sank at: a curve of the second degree, which as a cubic Bezier has
    // the control points (1/3, pace/3) and (2/3, (1 + pace)/3).
    let sink = space::verbs::fly::HELD_SINK;
    let pace = (sink * (1.0 - flying - holding)) / (holding * (1.0 - sink));
    let drop = Curve {
        x1: 1.0 / 3.0,
        y1: pace / 3.0,
        x2: 2.0 / 3.0,
        y2: (1.0 + pace) / 3.0,
    };
    out.segment(
        subject,
        Segment::between(lowered, to, part(step.own, flying + holding, 1.0), drop),
    );
}

/// How a curve of the second degree from `a` to `c` reaches `b` and no farther: its middle
/// control value (the web's `on` in `swept`). Where `b` lies beyond both ends the curve turns
/// back there, and where it lies between them it never turns.
fn pull(a: f32, b: f32, c: f32) -> f32 {
    let beyond = (b - a) * (b - c);
    if beyond > 0.0 {
        b + (b - a).signum() * beyond.sqrt()
    } else {
        b
    }
}

/// One of several cards drawn together, sweeping past where one alone is held up (the web's
/// `swept`): a curve of the second degree from `from` to `to` that goes as far across the
/// screen, as far up it, as near the eye and as large as that place, and no farther.
fn swept(step: &StepIn<'_>, (from, to): (Pose, Pose)) -> Shape {
    let via = held_place(step, to);
    let control = step
        .places
        .camera()
        .axes()
        .iter()
        .map(|&axis| {
            let along = |pose: Pose| pose.position.dot(axis);
            axis * pull(along(from), along(via), along(to))
        })
        .sum::<Vec3>();
    let scale = pull(from.scale, via.scale, to.scale);
    Shape::Swept {
        bulge: control - (from.position + to.position) / 2.0,
        grow: scale - f32::midpoint(from.scale, to.scale),
    }
}

pub fn play(step: &StepIn<'_>, out: &mut StepOut<'_>) {
    let (Some(subject), Some(from), Some(to)) = (step.subject, step.from, step.to) else {
        return;
    };
    let between = Segment::between(from, to, step.own, step.curve);
    let axes = step.places.camera().axes();
    let arc = |rises: bool| if rises { space::verbs::fly::ARC } else { 0.0 };
    let path = path_of(step.act, step.timing.case.as_deref());
    // A card whose face changes turns over at moments of its own: a drawn card as it leaves the
    // library, any other a little before half way (the web's `turningOf`).
    let turning = turns(from, to).then_some(if draws(step.act) {
        DRAW_TURN
    } else {
        CAST_TURN
    });
    let segment = match path {
        Path::Held => return held_up(step, (from, to), out, subject),
        Path::Swept => Segment {
            shape: swept(step, (from, to)),
            ..between
        },
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
            shape: Shape::Bowed(bow(bowing, toward, (from, to), step.places.camera())),
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
    out.segment(subject, Segment { turning, ..segment });
}
