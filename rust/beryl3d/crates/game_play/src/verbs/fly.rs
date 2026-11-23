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
//! - A card the viewer draws flies along a six-node Catmull-Rom spline with piecewise spherical
//!   rotation and a four-keyframe cubic Hermite timing curve (`Shape::Spline`): lifting off the
//!   library face down, flipping across edge-on near the middle of the table, leaning into a 3D
//!   flourish, holding large and square beside the stack, and dropping smoothly into the hand.
//! - The opponent's draw flies along a four-node Catmull-Rom spline (`Shape::Spline`): pausing
//!   briefly on the library, dropping flat down the flank, sweeping across upper centre while
//!   pitching halfway toward the camera, and landing face down in their hand.
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
//! `data/beryl3d/verbs.json`.
//!
//! The web turns a card that lands face down the other way up the table (the opponent's draw)
//! half round as it leaves, so that it never spins in the table's plane on the way. Beryl 3D's
//! opponent's hand faces the camera the same way up as the library, so the flight has no half
//! turn to take out.

use beryl_protocol::protocol::{Side, Why, ZoneName};
use beryl_protocol::script::Act;
use glam::{Quat, Vec2, Vec3};
use std::f32::consts::PI;

use super::{StepIn, StepOut};
use game_table::table::Camera;
use kit_base::data::profiles::HeldDraw;
use kit_base::data::verbs::{Bowing, Fly};
use kit_base::pose::{Bow, MAX_SPLINE_INTERIOR, Pose, Segment, Shape, Spline, SplineNode};

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
    /// A card drawn into the viewer's hand (single, multi-draw, or face down): six-node spline.
    DrawLocal,
    /// A card drawn into the opponent's hand: four-node spline.
    DrawOpponent,
}

/// Whether `act` is a card drawn, or a card flown as one is: the flight from a library to a
/// hand that the game calls a draw, or one no one at the table can read that an effect puts
/// there (the web's `draws` in `ui/src/stage/tracks.ts`).
fn draws(act: &Act) -> bool {
    matches!(act, Act::Fly { from: ZoneName::Library, why, object, .. }
        if object.zone == ZoneName::Hand && (*why == Why::Draw || object.name.is_none()))
}

/// The path of the flight `act` makes, whose case is `case` (the web's `bowOf` and `lifts`).
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
        Some("draw" | "fast draw" | "draw of several") => return Path::DrawLocal,
        Some("opponent draw") => {
            return if mine {
                Path::DrawLocal
            } else {
                Path::DrawOpponent
            };
        }
        _ => {}
    }
    if draws(act) {
        return if mine {
            Path::DrawLocal
        } else {
            Path::DrawOpponent
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

/// Moves `pose` in screen space at its current depth if any of its corners fall outside the
/// window's safe area.
fn clamped_to_safe_area(mut pose: Pose, camera: &Camera) -> Pose {
    let (corners, _) = camera.card_box(&pose);
    let inside = into_safe_area(&corners, camera);
    if inside != Vec2::ZERO {
        camera.move_in_screen(&mut pose, inside);
    }
    pose
}

/// Reference NDC endpoints of the local draw spline (library to hold spot) used to remap
/// interior waypoints proportionally on compact layouts where the hold spot sits above the hand.
const LOCAL_REF_FROM_NDC: Vec2 = Vec2::new(-0.74870, -0.81511);

/// Builds the six-node Catmull-Rom spline for a card drawn into the viewer's hand, projecting
/// each interior waypoint through the active camera and clamping it into the window's safe area.
fn local_draw_spline(step: &StepIn<'_>, (from, to): (Pose, Pose)) -> Shape {
    let spec = &step.verbs.fly.draw.local;
    let camera = step.places.camera();
    let camera_back = camera.axes()[2];
    let from_depth = camera.depth(from.position);
    let to_depth = camera.depth(to.position);
    let from_seen = from.scale / from_depth;
    let to_seen = to.scale / to_depth;
    let flips = turns(from, to);
    let stays_face_down = !flips && from.turn > 0.5;

    let (over_rest_ndc, hold_seen) = match step.places.layout().play.held_draw {
        HeldDraw::Fixed { size, .. } => (None, to_seen * size),
        HeldDraw::OverRest { .. } => {
            let hold = held_place(step, to);
            (
                Some(camera.project(hold.position).truncate()),
                hold.scale / camera.depth(hold.position),
            )
        }
    };
    let from_ndc = camera.project(from.position).truncate();
    let ref_hold_ndc = spec.nodes[3].ndc;
    let ref_span = ref_hold_ndc - LOCAL_REF_FROM_NDC;

    let mut interior = [SplineNode::default(); MAX_SPLINE_INTERIOR];
    for (index, node) in spec.nodes.iter().enumerate() {
        #[expect(clippy::cast_precision_loss, reason = "index is 0..4")]
        let u = (index + 1) as f32 / 5.0;
        let ndc = match over_rest_ndc {
            Some(hold_ndc) if index == 3 => hold_ndc,
            Some(hold_ndc) => {
                // Remap interior waypoints proportionally between `from_ndc` and `hold_ndc` on
                // narrow/compact layouts so the arc stays balanced over the screen.
                let share = (node.ndc - LOCAL_REF_FROM_NDC) / ref_span;
                from_ndc + (hold_ndc - from_ndc) * share
            }
            None => node.ndc,
        };
        let depth = (from_depth + (to_depth - from_depth) * node.depth_share).min(node.max_depth);
        let linear_seen = from_seen + (to_seen - from_seen) * u;
        let linear_rel = spec.from_rel_scale + (1.0 - spec.from_rel_scale) * u;
        let seen_scale = if index == 3 {
            hold_seen
        } else {
            linear_seen * (node.rel_scale / linear_rel)
        };
        let base = camera.screen_pose(ndc, depth, node.rotation, 0.1);
        let rotation = if flips {
            base.rotation
        } else if stays_face_down {
            from.rotation.slerp(to.rotation, u)
        } else if index == 0 {
            // An already face-up card lifts smoothly into the face-up flourish without turning
            // face down at node 1.
            let node2 = camera
                .screen_pose(spec.nodes[1].ndc, depth, spec.nodes[1].rotation, 0.1)
                .rotation;
            from.rotation.slerp(node2, 0.5)
        } else {
            base.rotation
        };
        let probe = Pose {
            position: base.position,
            rotation,
            scale: seen_scale * depth,
            turn: if flips { 0.0 } else { from.turn },
            ..Pose::default()
        };
        let clamped = clamped_to_safe_area(probe, camera);
        interior[index] = SplineNode {
            position: clamped.position,
            rotation,
            seen_scale,
            weight: 1.0 / node.rel_scale,
        };
    }

    Shape::Spline(Spline::new(
        &interior,
        1.0 / spec.from_rel_scale,
        &to,
        spec.curve(),
        (flips, false),
        (camera.eye, camera_back),
    ))
}

/// Builds the four-node Catmull-Rom spline for a card drawn into the opponent's hand: brief
/// launch pause, flat drop down the library's flank, rising sweep across upper centre, and
/// smooth arrival into the opponent's hand.
fn opponent_draw_spline(step: &StepIn<'_>, (from, to): (Pose, Pose)) -> Shape {
    let spec = &step.verbs.fly.draw.opponent;
    let camera = step.places.camera();
    let camera_back = camera.axes()[2];
    let from_depth = camera.depth(from.position);
    let to_depth = camera.depth(to.position);
    let from_seen = from.scale / from_depth;
    let to_seen = to.scale / to_depth;

    let from_ndc = camera.project(from.position).truncate();
    let to_ndc = camera.project(to.position).truncate();
    // On compact layouts where the opponent's library sits on the left and their hand on the
    // right, mirror the horizontal waypoints so the card drops cleanly down the library side.
    let mirror_x = if from_ndc.x < to_ndc.x - 0.05 {
        -1.0
    } else {
        1.0
    };
    // An inverted opponent hand fan turns its cards half round about local Z; turn the symmetric
    // card back half round at departure so it pitches cleanly without spinning in-plane.
    let start_half_turn = (from.rotation * Vec3::Y).dot(to.rotation * Vec3::Y) < 0.0;
    let start_rot = if start_half_turn {
        from.rotation * Quat::from_rotation_z(PI)
    } else {
        from.rotation
    };

    let mut interior = [SplineNode::default(); 2];
    for (index, node) in spec.nodes.iter().enumerate() {
        #[expect(clippy::cast_precision_loss, reason = "index is 0..2")]
        let u = (index + 1) as f32 / 3.0;
        let ndc = Vec2::new(node.ndc.x * mirror_x, node.ndc.y);
        let depth = from_depth + (to_depth - from_depth) * node.depth_share;
        let linear_seen = from_seen + (to_seen - from_seen) * u;
        let linear_rel = spec.from_rel_scale + (1.0 - spec.from_rel_scale) * u;
        let seen_scale = linear_seen * (node.rel_scale / linear_rel);
        let rotation = start_rot.slerp(to.rotation, node.pitch_blend);
        let base = camera.screen_pose(ndc, depth, Quat::IDENTITY, 0.1);
        let probe = Pose {
            position: base.position,
            rotation,
            scale: seen_scale * depth,
            turn: from.turn,
            ..Pose::default()
        };
        let clamped = clamped_to_safe_area(probe, camera);
        interior[index] = SplineNode {
            position: clamped.position,
            rotation,
            seen_scale,
            weight: 1.0 / node.rel_scale,
        };
    }

    Shape::Spline(Spline::new(
        &interior,
        1.0 / spec.from_rel_scale,
        &to,
        spec.curve(),
        (false, start_half_turn),
        (camera.eye, camera_back),
    ))
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
    // A card whose face changes turns over at moments of its own: a drawn card turns via its
    // 3D spline orientations, any other a little before half way (the web's `turningOf`).
    let turning = turns(from, to).then_some(if draws(step.act) {
        fly.turn.draw
    } else {
        fly.turn.cast
    });
    let segment = match path {
        Path::DrawLocal => {
            out.segment(
                subject,
                Segment {
                    shape: local_draw_spline(step, (from, to)),
                    ..between
                },
            );
            return;
        }
        Path::DrawOpponent => {
            out.segment(
                subject,
                Segment {
                    shape: opponent_draw_spline(step, (from, to)),
                    ..between
                },
            );
            return;
        }
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
                    * (fly.thrown.height * step.places.camera().data.card.width * from.scale)
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
