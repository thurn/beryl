//! The cards in each player's hand and the region the camera gives their fan.

use beryl_protocol::protocol::{ObjectView, Side, ZoneName};
use glam::{Quat, Vec2};
use kit_base::data::verbs::Lift;
use kit_base::pose::Pose;
use kit_space::arrange::{
    CentredFan, Container, Fan, Item, Limits, Policy, Region, Solved, arrange,
};

use super::place::{Placement, Rest, turn};
use super::{Camera, HandData, Layout, Place, RowWindow};

pub(super) fn hand(
    placement: &mut Placement,
    objects: &[&ObjectView],
    (side, scroll): (Side, usize),
    (layout, lift): (&Layout, &Lift),
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
    let (data, scroll) = if yours {
        (layout.hand, scroll)
    } else {
        (layout.opponent_hand, 0)
    };
    let strip = layout.strip;
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

    if let Some(decisions) = placement.decisions.as_mut() {
        decisions.hand[usize::from(!yours)] = Some(super::place::ContainerDecision {
            items: count + picked.len(),
            overflow: solved.overflow,
            press_points: if yours { strip } else { 0.0 },
        });
    }
    if yours {
        let (before, after) = (solved.before, solved.after);
        placement.rows.hand = RowWindow::new(
            before,
            count - before - after,
            before + after,
            (before, after),
        );
    }
    let place_card =
        |placement: &mut Placement, object: &ObjectView, laid: &kit_space::arrange::Placed| {
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
                    overlapped: false,
                },
            );
        };
    if yours {
        for (object, laid) in objects.iter().zip(&solved.items) {
            place_card(placement, object, laid);
        }
    } else {
        // Mirror the opponent's hand across the table so newly drawn cards enter the leftmost
        // slot, matching MTG Arena's across-the-table perspective.
        for (object, laid) in objects.iter().zip(solved.items.iter().rev()) {
            place_card(placement, object, laid);
        }
    }
    super::lifted::lay_picked(&picked, side, (layout, camera, lift), placement);
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
    let units = anchor.scale * camera.data.card.height;
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
            size: Vec2::new(camera.data.card.width, camera.data.card.height),
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
