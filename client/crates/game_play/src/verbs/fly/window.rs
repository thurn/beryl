//! Keeping a straight or bowed flight inside the window's safe area: as `fly` plans it
//! (`kept_in_window`), and again when the flight is made to start somewhere else than planned
//! (`start_at`).

use glam::{Vec2, Vec3};
use std::f32::consts::PI;

use super::{LOOKS, screen_way};
use game_table::table::Camera;
use kit_base::pose::{Bow, Pose, Segment, Shape};

/// Starts `segment` from `from` instead of the pose it was planned from, keeping its time,
/// curve and end (`Segment::start_at`). This is for a card a prompt's tray held up and let go
/// before the script that moves it began (`hand_over` in `play/tray.rs`).
///
/// A bow's offset is a length in millimetres, sized and turned for the way between the
/// planned ends as the window shows it (`bow`). The tray holds a card up close to the camera,
/// where a millimetre covers several times as many points as on the table, so on the phone
/// a bow kept as planned carried a card flying from the tray to the stack about 70 points past
/// the window's right edge. The offset is therefore turned and scaled with the way: it keeps
/// its parts along and across the way, as shares of the way's length on the window, and the
/// flight is then kept inside the window as `fly` keeps the flights it plans.
pub(crate) fn start_at(segment: &mut Segment, from: Pose, camera: &Camera, target: f32) {
    let Shape::Bowed(mut bow) = segment.shape else {
        segment.start_at(from);
        return;
    };
    let [right, up, back] = camera.axes();
    let planned = screen_way(segment.from.position, segment.to.position, camera);
    let way = screen_way(from.position, segment.to.position, camera);
    let across = |w: Vec2| Vec2::new(w.y, -w.x);
    let length = planned.length_squared();
    let on_window = Vec2::new(bow.offset.dot(right), bow.offset.dot(up));
    bow.offset = if length > f32::EPSILON {
        let along = on_window.dot(planned) / length;
        let aside = on_window.dot(across(planned)) / length;
        let turned = way * along + across(way) * aside;
        let depth = bow.offset.dot(back) * (way.length_squared() / length).sqrt();
        right * turned.x + up * turned.y + back * depth
    } else {
        Vec3::ZERO
    };
    segment.shape = Shape::Bowed(bow);
    segment.start_at(from);
    *segment = kept_in_window(*segment, camera, target);
}

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
pub(super) fn kept_in_window(segment: Segment, camera: &Camera, target: f32) -> Segment {
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
