//! The cards in each player's hand and the region the camera gives their fan.

use beryl_protocol::protocol::{ObjectView, Side, ZoneName};
use glam::{Quat, Vec2};
use kit_base::data::verbs::LiftHand;
use kit_base::pose::Pose;
use kit_frame::frame::Place;
use kit_space::arrange::{
    CentredFan, Container, Fan, Item, Limits, Policy, Region, Solved, arrange,
};

use super::place::{Placement, Rest, turn};
use super::{CARD_HEIGHT, CARD_WIDTH, Camera, HandData, RowWindow};

pub(super) fn hand(
    placement: &mut Placement,
    objects: &[&ObjectView],
    (side, scroll): (Side, usize),
    (data, strip, lift): (HandData, f32, &LiftHand),
    camera: &Camera,
    cache: Option<&mut HandFits>,
) {
    // A hidden pick separates its cards from the fan without changing the order of the others.
    let (picked, fan): (Vec<&ObjectView>, Vec<&ObjectView>);
    let objects = if objects.iter().any(|object| object.hidden_pick) {
        (picked, fan) = objects.iter().partition(|object| object.hidden_pick);
        &fan[..]
    } else {
        picked = Vec::new();
        objects
    };
    let count = objects.len();
    let place = Place::Card {
        zone: ZoneName::Hand,
        side,
        row: None,
    };
    let yours = side == Side::You;
    let scroll = if yours { scroll } else { 0 };
    let arrange = || fan_rests(count, yours, data, strip, camera, scroll);
    let fresh;
    let solved = if let Some(cache) = cache.filter(|_| count <= HandFits::MAX_CACHED && scroll == 0)
    {
        let fans = &mut cache.0[usize::from(!yours)];
        if fans.len() <= count {
            fans.resize_with(count + 1, || None);
        }
        fans[count].get_or_insert_with(arrange)
    } else {
        fresh = arrange();
        &fresh
    };

    if yours {
        let (before, after) = (solved.before, solved.after);
        placement.rows.hand =
            RowWindow::new(count, before, count - before - after, (before, after));
    }
    for (object, laid) in objects.iter().zip(&solved.items) {
        placement.set(
            object.slot,
            Rest {
                pose: Pose {
                    turn: turn(object),
                    ..laid.pose
                },
                place,
                visible: laid.in_view,
                copies: 1,
            },
        );
    }
    super::lifted::lay_picked(&picked, side, camera, data.ceiling, lift, placement);
}

/// Fan layouts by card count in a single script, whose camera and hand regions stay fixed.
#[derive(Default)]
pub struct HandFits([Vec<Option<Solved>>; 2]);

impl HandFits {
    const MAX_CACHED: usize = 32;
}

fn fan_rests(
    count: usize,
    yours: bool,
    data: HandData,
    strip: f32,
    camera: &Camera,
    scroll: usize,
) -> Solved {
    let anchor = camera.screen_pose(
        Vec2::new(data.across, data.edge),
        data.depth,
        Quat::IDENTITY,
        0.5,
    );
    let units = anchor.scale * CARD_HEIGHT;
    let window = camera.window.size;
    let region = Region {
        centre: anchor.position,
        rotation: anchor.rotation,
        size: units
            * Vec2::new(
                2.0 * window.width / window.height,
                data.ceiling.min(1.0) - data.edge,
            ),
    };
    let container = Container {
        region,
        policy: Policy::Fan(Fan::Centred(CentredFan {
            height: data.height,
            radius: data.fan_radius,
            step: data.spacing,
            span: data.room,
            bias: data.bias_degrees.to_radians(),
            inverted: !yours,
            depth_step: data.step_depth,
        })),
        limits: Limits {
            press_points: if yours { strip } else { 0.0 },
            overlap: true,
            scroll: yours,
            ..Limits::default()
        },
    };
    // While the opening deals the hand below the screen, its rests still guide the deal.
    let pressable = yours && data.edge >= -1.0;
    let items: Vec<_> = (0..count)
        .map(|index| Item {
            key: index as u64,
            size: Vec2::new(CARD_WIDTH, CARD_HEIGHT),
            pressable,
            group: None,
            under: None,
        })
        .collect();
    arrange(&container, &items, camera, scroll)
}

/// The top edge on the window, in logical points, of the viewer's hand at the resting places
/// `rests` give (each a place, a pose and whether it is visible), before any card is held up in
/// a tray; infinity for an empty hand. Allocates nothing.
pub(super) fn hand_top(camera: &Camera, rests: impl Iterator<Item = (Place, Pose, bool)>) -> f32 {
    let mut top = f32::INFINITY;
    for (place, pose, visible) in rests {
        if visible
            && let Place::Card {
                zone: ZoneName::Hand,
                side: Side::You,
                ..
            } = place
        {
            let corners = camera.card_box(&pose).0;
            top = corners.iter().fold(top, |top, corner| top.min(corner.y));
        }
    }
    top
}
