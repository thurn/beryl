//! The stack's placement: its spells and its abilities, bottom first, at the right of the screen
//! and placed relative to the camera, as the web interface's `stackPlace`
//! (`ui/src/game/stackLayout.ts`) places them (docs/beryl-3d.md, "Objects and placement").

use beryl_protocol::protocol::{ObjectId, ObjectView, Side, StackItem, ZoneName};

use glam::{Quat, Vec2};

use super::place::{Placement, Rest, turn};
use super::{CARD_HEIGHT, CARD_WIDTH, Camera, Layout, RowWindow, StackData, Table};
use kit_base::data::audit;
use kit_base::pose::Pose;
use kit_frame::frame::Place;
use kit_space::arrange::{AnchoredCascade, Cascade, CascadeLayout};

/// An item of the stack as placement reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnStack {
    /// The spell whose card is in this slot.
    Spell(ObjectId),
    /// The ability with this number, on its controller's side.
    Ability { id: u32, side: Side },
    /// An ability nothing numbered, which is not placed but takes its place on the stack, so
    /// that the items above it stand where they would if it were numbered.
    Unnumbered,
}

/// The items of `stack`, in order, each given with its number if it has one. An ability with
/// none takes the next of `numbers`, and one with neither is unnumbered. A spell resolving is
/// the top item: the game takes it off the stack as it begins to resolve, but its card's slot
/// stays among `stack_slots` until the card leaves the stack zone.
pub fn on_stack<'a>(
    stack: impl Iterator<Item = (&'a StackItem, Option<u32>)>,
    numbers: &[u32],
    stack_slots: impl Iterator<Item = ObjectId>,
) -> Vec<OnStack> {
    let mut numbers = numbers.iter().copied();
    let mut items: Vec<OnStack> = stack
        .map(|(item, id)| match item {
            StackItem::Spell { slot, .. } => OnStack::Spell(*slot),
            StackItem::Ability { controller, .. } => match id.or_else(|| numbers.next()) {
                Some(id) => OnStack::Ability {
                    id,
                    side: *controller,
                },
                None => OnStack::Unnumbered,
            },
        })
        .collect();
    for slot in stack_slots {
        if !items.contains(&OnStack::Spell(slot)) {
            items.push(OnStack::Spell(slot));
        }
    }
    items
}

/// The slots of the spells on `stack`, in order.
pub(super) fn spells(stack: &[OnStack]) -> Vec<ObjectId> {
    stack
        .iter()
        .filter_map(|item| match item {
            OnStack::Spell(slot) => Some(*slot),
            OnStack::Ability { .. } | OnStack::Unnumbered => None,
        })
        .collect()
}

/// The stack's items, `stack` bottom first, at the right of the screen: the top item in front,
/// and each item beneath it peeking out above and to its left. A spell is its card, one of
/// `spells`; an ability is drawn as a card too, showing its source's face, and rests by number
/// in `placement.abilities`.
pub(super) fn place_stack(
    placement: &mut Placement,
    spells: &[&ObjectView],
    stack: &[OnStack],
    (layout, camera, scroll): (&Layout, &Camera, usize),
) {
    let count = stack.len();
    let (arranged, units) = arrange(layout, camera, (count, scroll));
    let (depth, step_depth) = layout
        .phone
        .map_or((layout.stack.depth, layout.stack.step_depth), |phone| {
            (phone.depth, phone.step_depth)
        });
    let before = count - arranged.window.first - arranged.window.shown;
    placement.rows.stack = RowWindow::new(
        count,
        before,
        arranged.window.shown,
        (before, arranged.window.first),
    );
    let window = camera.window.size;
    let step_depth = step_depth.min(depth / (2.0 * count.max(1) as f32));
    let screen_pose = |index| {
        let (centre, visible) = arranged.item(count - 1 - index);
        let (ndc, height) = match units {
            ProjectedUnits::Points => (
                Vec2::new(
                    centre.x / window.width * 2.0 - 1.0,
                    1.0 - centre.y / window.height * 2.0,
                ),
                arranged.size.y / window.height,
            ),
            ProjectedUnits::HalfWindowHeight => (
                Vec2::new(centre.x * window.height / window.width, centre.y),
                arranged.size.y / 2.0,
            ),
        };
        (
            camera.screen_pose(
                ndc,
                depth - index as f32 * step_depth,
                Quat::IDENTITY,
                height,
            ),
            visible,
        )
    };
    placement.abilities.clear();
    // `spells` lists the spells of `stack` in its order, leaving out any whose card the table
    // does not hold.
    let mut spells = spells.iter().peekable();
    for (index, item) in stack.iter().enumerate() {
        let (turn, side) = match *item {
            OnStack::Spell(slot) => {
                let Some(object) = spells.next_if(|object| object.slot == slot) else {
                    continue;
                };
                (turn(object), object.controller)
            }
            OnStack::Ability { side, .. } => (0.0, side),
            OnStack::Unnumbered => continue,
        };
        let (pose, visible) = screen_pose(index);
        let rest = Rest {
            pose: Pose { turn, ..pose },
            place: Place::Card {
                zone: ZoneName::Stack,
                side,
                row: None,
            },
            visible,
            copies: 1,
        };
        match *item {
            OnStack::Spell(slot) => placement.set(slot, rest),
            OnStack::Ability { id, .. } => placement.abilities.push((id, rest)),
            OnStack::Unnumbered => {}
        }
    }
    placement.abilities.sort_unstable_by_key(|(id, _)| *id);
}

enum ProjectedUnits {
    /// Logical points with the origin at the top left.
    Points,
    /// Half the window's height with the origin at the centre and y pointing up.
    HalfWindowHeight,
}

fn arrange(
    layout: &Layout,
    camera: &Camera,
    items: (usize, usize),
) -> (CascadeLayout, ProjectedUnits) {
    if let Some(data) = layout.phone {
        let placed = Cascade {
            direction: Vec2::NEG_X,
            step: data.step,
        }
        .centred(
            (data.left, data.right, data.middle),
            Vec2::new(data.width(), data.height),
            (audit::PHONE_TARGET_POINTS, data.count_room),
            items,
        );
        (placed, ProjectedUnits::Points)
    } else {
        let (cascade, region, target) = anchored(&layout.stack, camera);
        (
            cascade.anchored(region, target, items),
            ProjectedUnits::HalfWindowHeight,
        )
    }
}

impl Table {
    /// The box of the stack's visible window, in logical points from the screen's top left.
    #[must_use]
    pub fn stack_bounds(&self) -> Option<(Vec2, Vec2)> {
        let window = self.rows().stack;
        let (arranged, units) = arrange(
            self.layout(),
            self.camera(),
            (window.groups(), window.after()),
        );
        let (low, high) = arranged.bounds()?;
        Some(match units {
            ProjectedUnits::Points => (low, high),
            ProjectedUnits::HalfWindowHeight => to_screen_bounds(self.camera(), (low, high)),
        })
    }
}

/// Fit the camera-facing region in units of half the window height, with its front
/// card anchored at the right and enough extra room on the left to wrap a tall stack.
fn anchored(data: &StackData, camera: &Camera) -> (Cascade, AnchoredCascade, f32) {
    let window = camera.window;
    let units = 2.0 / window.size.height;
    let aspect = window.size.width / window.size.height;
    let size = Vec2::new(CARD_WIDTH / CARD_HEIGHT, 1.0) * data.height * 2.0;
    let target = audit::TARGET_POINTS * units;
    (
        Cascade {
            direction: Vec2::new(-data.lean, 1.0),
            step: data.step,
        },
        AnchoredCascade {
            foot: Vec2::new(aspect - data.right - size.x / 2.0, data.foot),
            ceiling: data.top,
            left: -aspect + window.safe_area.left * units,
            size,
            reach: data.reach,
            pitch: target * data.spare,
            least_scale: 0.4,
        },
        target,
    )
}

/// The stack's steady clearance rectangle in logical points, from top left to bottom right.
pub(super) fn reservation(data: &StackData, camera: &Camera) -> (Vec2, Vec2) {
    let (cascade, region, _) = anchored(data, camera);
    to_screen_bounds(camera, cascade.reservation(region))
}

fn to_screen_bounds(camera: &Camera, (low, high): (Vec2, Vec2)) -> (Vec2, Vec2) {
    let window = camera.window.size;
    let half = window.height / 2.0;
    (
        Vec2::new(window.width / 2.0 + low.x * half, (1.0 - high.y) * half),
        Vec2::new(window.width / 2.0 + high.x * half, (1.0 - low.y) * half),
    )
}
