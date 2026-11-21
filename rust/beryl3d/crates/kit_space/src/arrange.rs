//! Arranges rectangles in a plane. The caller chooses the region, policy, item order and
//! touch target. Arrangement is computed from those inputs, the camera and the scroll offset.

use std::collections::BTreeMap;

use glam::{Quat, Vec2, Vec3};
use kit_base::pose::Pose;

use crate::camera::Camera;

mod layout;
mod press;

use layout::Layout;

/// The available rectangle in table space. The rotation takes the region's local right,
/// up and front axes into table space. Sizes in the region are in millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    pub centre: Vec3,
    pub size: Vec2,
    pub rotation: Quat,
}

impl Region {
    fn point(self, at: Vec2) -> Vec3 {
        self.centre + self.rotation * at.extend(0.0)
    }
}

/// The fixed origin of a row or fan. Middle starts at the region's centre and alternates
/// right and left, so removing an item does not recenter the items before it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Alignment {
    #[default]
    Start,
    Middle,
    End,
}

/// The side of a host from which a tucked item shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Start,
    End,
}

/// A rectangle placed behind its host, with its exposed edge on the given side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Under {
    pub host: u64,
    pub edge: Edge,
}

/// Related items can stand together without being visually interchangeable. The caller
/// supplies an identity only when one item can represent the others with that identity
/// in this group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Group {
    pub key: u64,
    pub identical: Option<u64>,
}

/// One rectangle, at its natural width and height in millimetres. Keys are unique within a
/// container. A tucked item names a root item in the same container. An absent host, a
/// self-reference or a reference to another tucked item leaves the item on its own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Item {
    pub key: u64,
    pub size: Vec2,
    pub pressable: bool,
    /// Consecutive roots in the same group share their ordinary spacing.
    pub group: Option<Group>,
    pub under: Option<Under>,
}

/// The way the container places its items in the region's local plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Policy {
    /// Side by side. `group_gap` is added between distinct groups. Successive items under
    /// the same side of a host are offset by `tuck_step` per item of depth.
    Row {
        alignment: Alignment,
        gap: f32,
        group_gap: f32,
        tuck_step: Vec2,
    },
    /// A circular arc of radius `radius`, tangent to the horizontal at the middle.
    /// Each rectangle turns to follow the arc, up to `max_lean` radians.
    Fan {
        alignment: Alignment,
        radius: f32,
        max_lean: f32,
        gap: f32,
    },
    /// A step along `direction`, with `step` as the preferred distance between successive advances.
    Cascade { direction: Vec2, step: f32 },
    /// One stack, with its first item on top. The following nonpressable items are
    /// layered below it with this offset. A separately pressable item starts another stack.
    Pile { offset: Vec2 },
    /// From the top left, by rows of at most `columns` items.
    Grid { columns: usize, gap: Vec2 },
    /// A single rectangle centred in the region.
    Spot,
}

/// Which overflow steps a container allows. A scale of 1 disables shrinking.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    pub press_points: f32,
    pub pile_identical: bool,
    pub overlap: bool,
    pub least_scale: f32,
    pub scroll: bool,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            press_points: 24.0,
            pile_identical: false,
            overlap: false,
            least_scale: 1.0,
            scroll: false,
        }
    }
}

/// A region, its placement policy, and the overflow steps it permits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Container {
    pub region: Region,
    pub policy: Policy,
    pub limits: Limits,
}

/// The last overflow step needed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overflow {
    FullSize,
    Piled,
    Overlapped,
    Scaled,
    Scrolled,
    /// Only the fitting prefix is shown because scrolling is disabled.
    Clipped,
}

/// The result for one input item. In-view items are drawn in the selected window,
/// including any layers fully covered by other layers. A hidden item has a rest pose at its cluster's
/// resting place or at the end of the window. A gathered copy has count zero.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub key: u64,
    pub pose: Pose,
    pub in_view: bool,
    pub count: usize,
    /// Back-to-front order within this container. Draw or hit-test higher layers first
    /// when considering what covers a lower layer.
    pub layer: usize,
    /// A square of `press_points` logical points per side on screen, turned with the
    /// item. Present for each visible pressable item when the target is positive.
    pub press_square: Option<[Vec2; 4]>,
}

/// The arranged items in input order, and the number of input items outside each
/// end of the window, including tucked items and gathered copies.
#[derive(Clone, Debug, PartialEq)]
pub struct Solved {
    pub items: Vec<Placed>,
    pub overflow: Overflow,
    pub before: usize,
    pub after: usize,
}

/// A cluster advances as one unit. Copies rest at the face's pose. Members are drawn
/// underneath the face, nearest to it first.
#[derive(Debug)]
struct Cluster {
    face: usize,
    copies: Vec<usize>,
    members: Vec<usize>,
}

impl Cluster {
    fn new(face: usize) -> Self {
        Self {
            face,
            copies: Vec::new(),
            members: Vec::new(),
        }
    }

    fn len(&self) -> usize {
        1 + self.copies.len() + self.members.len()
    }
}

fn clusters(items: &[Item], policy: Policy, pile_identical: bool) -> Vec<Cluster> {
    let keys: BTreeMap<_, _> = items
        .iter()
        .enumerate()
        .map(|(i, item)| (item.key, i))
        .collect();
    let host = |i: usize| {
        let under = items[i].under?;
        let &host = keys.get(&under.host)?;
        (items[host].under.is_none() && matches!(policy, Policy::Row { .. })).then_some(host)
    };
    let mut attached = vec![Vec::new(); items.len()];
    for i in 0..items.len() {
        if let Some(host) = host(i) {
            attached[host].push(i);
        }
    }
    let mut identities = BTreeMap::new();
    let mut clusters: Vec<Cluster> = Vec::new();
    for (i, item) in items.iter().enumerate().filter(|(i, _)| host(*i).is_none()) {
        if !item.pressable
            && matches!(policy, Policy::Pile { .. })
            && let Some(top) = clusters.last_mut()
        {
            top.members.push(i);
            continue;
        }
        let identity = item
            .group
            .and_then(|group| Some((group.key, group.identical?)));
        if pile_identical
            && attached[i].is_empty()
            && let Some(identity) = identity
        {
            let key = (
                identity,
                item.size.x.to_bits(),
                item.size.y.to_bits(),
                item.pressable,
            );
            if let Some(&representative) = identities.get(&key) {
                let cluster: &mut Cluster = &mut clusters[representative];
                cluster.copies.push(i);
                continue;
            }
            identities.insert(key, clusters.len());
        }
        let mut cluster = Cluster::new(i);
        cluster.members = std::mem::take(&mut attached[i]);
        clusters.push(cluster);
    }
    clusters
}

/// Arrange the items, taking the first allowed overflow stage that fits. The scroll offset
/// is the number of clusters to skip. A window reaching the end is filled back towards the
/// start. A cluster that cannot fit alone can be passed by scrolling.
///
/// Within a scale and overflow stage, the policies arrange from fixed origins in input order.
/// Removing a later cluster leaves earlier positions stable. Tucked items share a cluster
/// with their host. If the cluster's footprint changes, its contents are arranged together.
/// A change of scale, piling stage or visible window can reflow the container.
#[must_use]
pub fn arrange(container: &Container, items: &[Item], camera: &Camera, scroll: usize) -> Solved {
    let layout = Layout::new(container, items, camera);
    let mut clusters = clusters(items, container.policy, false);
    if let Ok(placed) = layout.fit(&clusters, 1.0, false) {
        return layout.solved(&clusters, (0, clusters.len()), placed, Overflow::FullSize);
    }
    if container.limits.pile_identical {
        clusters = self::clusters(items, container.policy, true);
        if let Ok(placed) = layout.fit(&clusters, 1.0, false) {
            return layout.solved(&clusters, (0, clusters.len()), placed, Overflow::Piled);
        }
    }
    let overlap = container.limits.overlap;
    if overlap && let Ok(placed) = layout.fit(&clusters, 1.0, true) {
        return layout.solved(&clusters, (0, clusters.len()), placed, Overflow::Overlapped);
    }
    let least = container.limits.least_scale.clamp(f32::EPSILON, 1.0);
    if least < 1.0
        && let Some((_, placed)) = layout.shrink(&clusters, least, overlap)
    {
        return layout.solved(&clusters, (0, clusters.len()), placed, Overflow::Scaled);
    }
    layout.window(&clusters, least, overlap, scroll)
}
