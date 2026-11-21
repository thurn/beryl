//! `fly`: the card travels from where it is to where it rests, turning over on the way if its face
//! changes. The flight's path depends on what the card is doing, as the web interface's `bowOf`
//! (`ui/src/stage/bows.ts`) and `lifts` (`ui/src/stage/lift.ts`) pick it:
//!
//! - A spell cast onto the stack bows off the straight line, sideways on the screen, toward its
//!   controller's edge of the table, the opponent's half as far and later, leaning the other way.
//! - A permanent spell resolving is thrown up off the stack and drops onto its place
//!   (`Shape::Thrown`), rising no higher than the top of the window leaves room for.
//! - The viewer's card played from their hand onto the table, such as a land, glides straight
//!   to its place. The scene draws it in front of the resting cards while it travels.
//! - The opponent's land drops out of their hand into the open table on the side away from
//!   where it is going, tilted, and swings back along the table to its place.
//! - A card the viewer draws alone flies from the library to a place beside the stack, bowing
//!   up the screen and leaning a little as it lifts off, is held up there large to be read,
//!   sinking a little toward the hand, and drops into the hand (the web's `heldUp` in `ui/src/stage/drawn.ts`). Each of several drawn
//!   together sweeps past that place without stopping (`swept`).
//! - The opponent's draw drops toward the middle of the table and rises into their hand, and so
//!   does a card no one can read that an effect puts in a hand from a library, farther.
//! - An opponent's discard drops along the line from hand to graveyard. Other flights also
//!   follow a straight line across the table.
//! - Any straight or bowed flight that would carry its card's centre past the top, bottom or
//!   right edge of the window is bowed down, up or left on the screen until it does not
//!   (`kept_in_window`): the opponent's hand and their piles all lie along the top of the
//!   window, and the viewer's piles reach its bottom edge, so a spell cast from the viewer's
//!   graveyard, which bows toward the viewer's edge, would dip below it.
//!
//! Flights that rise use `verbs.fly.arc` for their height at the middle of the step: every
//! flight but a card's played or cast from the viewer's hand, a drawn card's or an opponent's
//! discard. A card whose face changes turns over top over bottom at moments of its own
//! (`Turning`): a drawn card as it leaves the library, any other a little
//! before half way. A card drawn alone turns at `held.turn` of its flight in, and grows on the
//! screen toward its held size only once it is edge-on. The values are under `verbs.fly` in
//! `data/beryl3d/space.json`.
//!
//! The web turns a card that lands face down the other way up the table (the opponent's draw)
//! half round as it leaves, so that it never spins in the table's plane on the way. Beryl 3D's
//! opponent's hand faces the camera the same way up as the library, so the flight has no half
//! turn to take out.

use beryl_protocol::protocol::{Side, Why, ZoneName};
use beryl_protocol::script::Act;
use glam::{Quat, Vec2, Vec3};
use std::f32::consts::PI;

use super::{Interval, StepIn, StepOut};
use beryl_protocol::script::Subject;
use game_table::table::Camera;
use kit_base::data::profiles::HeldDraw;
use kit_base::data::space;
use kit_base::data::verbs::{Bowing, Fly};
use kit_base::pose::{Bow, Curve, Pose, Segment, Shape};

pub const BUILT: bool = true;

/// The path a flight takes.
enum Path {
    /// The straight line, rising `verbs.fly.arc` at its middle when `rises`.
    Straight {
        rises: bool,
    },
    /// Bowed toward one edge of the table: `toward` is 1 for the viewer's edge and -1 for the
    /// opponent's, as the web's `toward` is 1 down the stage.
    Bowed {
        bowing: Bowing,
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
fn path_of(act: &Act, case: Option<&str>, fly: &Fly) -> Path {
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
            bowing: if *why == Why::Draw {
                fly.bows.drawn
            } else {
                fly.bows.taken
            },
            toward: -toward,
            rises: true,
        };
    }
    match (*from, object.zone) {
        // A spell cast from a hand goes between two places held near the camera, and never
        // leaves the table to rise above it: a rise along the table's height would carry it up
        // the screen against its bow.
        (_, ZoneName::Stack) => Path::Bowed {
            bowing: if mine { fly.bows.cast } else { fly.bows.theirs },
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
            bowing: fly.bows.played,
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

/// How far, in millimetres up the screen, a card can rise from `from` or `to`, whichever has
/// less room, before its top passes the top of the window's safe area (the web's `room` in
/// `lifted`, `ui/src/stage/lift.ts`, which keeps a permanent resolving into the opponent's back
/// row from rising past the far edge of the table). Each is measured at that pose's depth.
fn room((from, to): (Pose, Pose), camera: &Camera) -> f32 {
    let right = camera.axes()[0];
    let top = camera.window.safe_area.top;
    [from, to]
        .map(|pose| {
            let (corners, _) = camera.card_box(&pose);
            let highest = corners.iter().map(|c| c.y).fold(f32::INFINITY, f32::min);
            let points =
                camera.to_screen(pose.position + right).x - camera.to_screen(pose.position).x;
            ((highest - top) / points).max(0.0)
        })
        .into_iter()
        .fold(f32::INFINITY, f32::min)
}

/// Where a drawn card is held up to be read. A fixed position uses the hand's card height.
/// A position above the resting card uses that card's projected height. The pose is moved
/// inside the safe area if any corner would fall outside.
fn held_place(step: &StepIn<'_>, rest: Pose) -> Pose {
    let held = &step.verbs.fly.held;
    let camera = step.places.camera();
    let layout = step.places.layout();
    let size = camera.window.size;
    let (mut ndc, height) = match layout.play.held_draw {
        HeldDraw::Fixed { ndc, size } => (ndc, layout.hand.height * size),
        HeldDraw::OverRest { size: scale, foot } => {
            let (corners, _) = camera.card_box(&rest);
            let low = corners
                .iter()
                .copied()
                .reduce(Vec2::min)
                .unwrap_or_default();
            let high = corners
                .iter()
                .copied()
                .reduce(Vec2::max)
                .unwrap_or_default();
            let tall = (high.y - low.y) * scale;
            let middle = Vec2::new(
                f32::midpoint(low.x, high.x),
                low.y + (high.y - low.y) * foot - tall / 2.0,
            );
            (
                Vec2::new(
                    middle.x / (size.width / 2.0) - 1.0,
                    1.0 - middle.y / (size.height / 2.0),
                ),
                tall / size.height,
            )
        }
    };
    let place = |ndc: Vec2| camera.screen_pose(ndc, held.depth, Quat::IDENTITY, height);
    let mut pose = place(ndc);
    // The card faces the camera at a fixed depth, so it moves on the window as its `ndc` does,
    // one window's half width or height for each unit.
    let (corners, _) = camera.card_box(&pose);
    let inside = into_safe_area(&corners, camera);
    if inside != Vec2::ZERO {
        ndc += Vec2::new(
            inside.x / (size.width / 2.0),
            -inside.y / (size.height / 2.0),
        );
        pose = place(ndc);
    }
    Pose {
        turn: rest.turn,
        opacity: rest.opacity,
        ..pose
    }
}

/// How far, in points on the window, a box with `corners` must move to lie inside the window's
/// safe area along each axis, or as far in as it can where it is larger: zero where it already
/// does.
fn into_safe_area(corners: &[Vec2], camera: &Camera) -> Vec2 {
    let (size, inset) = (camera.window.size, camera.window.safe_area);
    let low = corners
        .iter()
        .copied()
        .reduce(Vec2::min)
        .unwrap_or_default();
    let high = corners
        .iter()
        .copied()
        .reduce(Vec2::max)
        .unwrap_or_default();
    let inward = |low: f32, high: f32, start: f32, end: f32| {
        if high > end {
            (end - high).max(start - low)
        } else if low < start {
            start - low
        } else {
            0.0
        }
    };
    Vec2::new(
        inward(low.x, high.x, inset.left, size.width - inset.right),
        inward(low.y, high.y, inset.top, size.height - inset.bottom),
    )
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
        skew: from.skew + (to.skew - from.skew) * u,
        shear: from.shear + (to.shear - from.shear) * u,
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
/// step: the flight in to where it is held, `held.in` of the step, bowed up the screen and, if
/// its face changes, turning over as it leaves the library at `held.turn` of the flight in,
/// leaning `held.lean` degrees as it lifts off and growing on the screen only once it is
/// edge-on (`Shape::Held`); the hold, `held.hold` of the step, sinking evenly by
/// `held_sink` of the way toward the hand; and the drop into the hand, leaving that pace and
/// quickening, with no jolt between the two.
fn held_up(step: &StepIn<'_>, (from, to): (Pose, Pose), out: &mut StepOut<'_>, subject: Subject) {
    let fly = &step.verbs.fly;
    let held = &fly.held;
    let up = held_place(step, to);
    let lowered = lerp(up, to, fly.held_sink);
    let (flying, holding) = (held.r#in, held.hold);
    let camera = step.places.camera();
    let bowed = bow(&held.bow, -1.0, (from, up), camera);
    // The web leans and holds back the growth only of a card that turns over (its `heldUp`
    // returns the bowed pose alone for a card already face up).
    let (shape, turning) = if turns(from, to) {
        let lean = held.lean.to_radians();
        let eye = camera.eye.dot(bowed.axis);
        (
            Shape::Held {
                bow: bowed,
                lean,
                eye,
            },
            Some(held.turn),
        )
    } else {
        (Shape::Bowed(bowed), None)
    };
    out.segment(
        subject,
        Segment {
            shape,
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
    let sink = fly.held_sink;
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
    let fly = &step.verbs.fly;
    let (Some(subject), Some(from), Some(to)) = (step.subject, step.from, step.to) else {
        return;
    };
    let between = Segment::between(from, to, step.own, step.curve);
    let axes = step.places.camera().axes();
    // A card that flies to the place it already rests rises no more than it travels: one out of
    // view at a row's end that moves to the end of that row stays there, undrawn.
    let travels = from.position.truncate() != to.position.truncate();
    let arc = |rises: bool| {
        if rises && travels { fly.arc } else { 0.0 }
    };
    let path = path_of(step.act, step.timing.case.as_deref(), fly);
    // A card whose face changes turns over at moments of its own: a drawn card as it leaves the
    // library, any other a little before half way (the web's `turningOf`).
    let turning = turns(from, to).then_some(if draws(step.act) {
        fly.turn.draw
    } else {
        fly.turn.cast
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
            shape: Shape::Bowed(bow(&bowing, toward, (from, to), step.places.camera())),
            ..between
        },
        Path::Thrown => Segment {
            shape: Shape::Thrown {
                // Up the screen by a share of the spell's width, and no higher than the room
                // over it.
                rise: axes[1]
                    * (fly.thrown.height * space::card::WIDTH * from.scale)
                        .min(room((from, to), step.places.camera())),
                aloft: fly.thrown.scale * from.scale,
                tuning: fly.thrown,
            },
            ..between
        },
    };
    let segment = kept_in_window(
        Segment { turning, ..segment },
        step.places.camera(),
        step.places.layout().target,
    );
    out.segment(subject, segment);
}

/// How many moments of a flight `kept_in_window` looks at.
const LOOKS: u16 = 24;

/// How much of its bow a moment must have for `kept_in_window` to push the card from there:
/// near either end a bow moves the card hardly at all, and the ends are where the card rests.
const PUSHED_FROM: f32 = 0.2;

/// `segment`, bowed down, up or left on the screen as far as it takes for its card's centre to
/// stay inside the top, bottom and right edges of the window's safe area, by half the side of the square the
/// "on screen" check asks to see of a card, so that at least half the card and all that square
/// are seen, at every moment of its flight but those near its ends. The opponent's hand is held
/// mostly above the window, and their graveyard, exile and library lie along the top right, so a
/// flight between them that keeps to its own line in the table's space is mostly out of sight:
/// its ends lie at very different depths, so that line curves above the top edge on the window.
/// A segment whose card stays inside comes back as it was. Only straight and bowed paths are
/// pushed, since the others already keep to the window by their own rules.
fn kept_in_window(segment: Segment, camera: &Camera, target: f32) -> Segment {
    let [right, up, back] = camera.axes();
    let mut bow = match segment.shape {
        Shape::Line => Bow {
            offset: Vec3::ZERO,
            sharpness: 1.0,
            lean: 0.0,
            axis: back,
        },
        Shape::Bowed(bow) => bow,
        _ => return segment,
    };
    let with = |bow: Bow| Segment {
        shape: Shape::Bowed(bow),
        ..segment
    };
    let (size, insets) = (camera.window.size, camera.window.safe_area);
    // Half the square of a card the "on screen" check holds inside the safe area, so that the
    // centre kept that far in leaves the whole square in.
    let margin = target / 2.0;
    // The moments looked at, each as the card's centre with no bow and the share of the bow it
    // has there, found once, since a push changes only the bow's offset. They are spread evenly
    // over the easing curve's own parameter rather than over time, which takes no search for
    // the progress at a given time (`Curve::ease`), a cost paid for every flight that starts.
    // Each centre is projected alone, rather than the card's four corners.
    let way = segment.to.position - segment.from.position;
    let mut moments = [(Vec3::ZERO, 0.0_f32); LOOKS as usize];
    let mut count = 0;
    for look in 1..LOOKS {
        let [elapsed, eased] = segment.curve.point(f32::from(look) / f32::from(LOOKS));
        let arch = bow.arch(eased);
        if arch >= PUSHED_FROM {
            let rise = Vec3::Z * segment.arc * (PI * elapsed).sin();
            moments[count] = (segment.from.position + way * eased + rise, arch);
            count += 1;
        }
    }
    let mut pushed = false;
    // The window's projection is not linear in depth, so a push is measured again after it is
    // made, and made again for what is still short.
    for _ in 0..3 {
        let (mut down, mut upward, mut left) = (0.0_f32, 0.0_f32, 0.0_f32);
        for &(straight, arch) in &moments[..count] {
            let position = straight + bow.offset * arch;
            let centre = camera.to_screen(position);
            let points = camera.to_screen(position + Vec3::X).x - centre.x;
            let short_down = insets.top + margin - centre.y;
            let short_up = centre.y - (size.height - insets.bottom - margin);
            let short_left = centre.x - (size.width - insets.right - margin);
            down = down.max(short_down / points / arch);
            upward = upward.max(short_up / points / arch);
            left = left.max(short_left / points / arch);
        }
        if down <= 0.0 && upward <= 0.0 && left <= 0.0 {
            break;
        }
        bow.offset += up * (upward.max(0.0) - down.max(0.0)) - right * left.max(0.0);
        pushed = true;
    }
    if pushed { with(bow) } else { segment }
}
