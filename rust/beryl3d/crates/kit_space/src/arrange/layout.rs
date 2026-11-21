use std::collections::BTreeMap;

use glam::{Quat, Vec2};
use kit_base::geometry::{GroupWindow, ScrollAxis};

use super::press::{self, PressArea, Quad};
use super::{
    Alignment, Camera, Cluster, Container, Edge, Fan, Item, Overflow, Pile, Placed, Policy, Pose,
    Solved, spot,
};

/// Bisection of projected strips and layout scale. Twelve steps resolve a typical item
/// within a fraction of a logical point.
const FIT_STEPS: usize = 12;

pub(super) struct Layout<'a> {
    container: &'a Container,
    items: &'a [Item],
    camera: &'a Camera,
    screen: Quad,
}

#[derive(Clone, Copy)]
struct Rect {
    centre: Vec2,
    half: Vec2,
    angle: f32,
}

impl Rect {
    fn corners(self) -> [Vec2; 4] {
        let turn = Vec2::from_angle(self.angle);
        let Vec2 { x, y } = self.half;
        [(-x, -y), (x, -y), (x, y), (-x, y)]
            .map(|(x, y)| self.centre + turn.rotate(Vec2::new(x, y)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FitError {
    OutsideRegion,
    Unreachable,
}

struct Surface {
    index: usize,
    pose: Pose,
    quad: Quad,
    press_square: Option<[Vec2; 4]>,
}

/// A cluster in local coordinates, with the face last so it covers its members.
struct Pack {
    parts: Vec<(usize, Rect)>,
    low: Vec2,
    high: Vec2,
}

impl Pack {
    fn size(&self) -> Vec2 {
        self.high - self.low
    }

    fn reach(&self, direction: Vec2) -> f32 {
        direction.abs().dot(self.size())
    }

    /// Distance from the host's centre to the first edge along the line.
    fn leading(&self, direction: Vec2) -> f32 {
        direction.max(Vec2::ZERO).dot(self.low) + direction.min(Vec2::ZERO).dot(self.high)
    }
}

#[derive(Clone, Copy)]
struct Line {
    direction: Vec2,
    edge: Vec2,
}

impl Line {
    /// A line from the corresponding edge or corner of the region.
    fn start(half: Vec2, direction: Vec2) -> Self {
        let sign = Vec2::new(direction.x.signum(), direction.y.signum());
        let edge = Vec2::select(direction.cmpne(Vec2::ZERO), -half * sign, Vec2::ZERO);
        Self { direction, edge }
    }

    fn centre(self, pack: &Pack) -> Vec2 {
        // A diagonal starts in a corner. Align both leading edges there, then advance along
        // the line. Horizontal and vertical lines keep the cross axis at the region's middle.
        let positive = self.direction.cmpgt(Vec2::ZERO);
        let nonzero = self.direction.cmpne(Vec2::ZERO);
        self.edge
            - Vec2::select(
                nonzero,
                Vec2::select(positive, pack.low, pack.high),
                Vec2::ZERO,
            )
    }

    fn advance(&mut self, by: f32) {
        self.edge += self.direction * by;
    }
}

impl<'a> Layout<'a> {
    pub(super) fn new(container: &'a Container, items: &'a [Item], camera: &'a Camera) -> Self {
        Self {
            container,
            items,
            camera,
            screen: press::safe_area(camera),
        }
    }

    fn pack(&self, cluster: &Cluster, scale: f32) -> Pack {
        let rectangle = |index: usize, centre| Rect {
            centre,
            half: self.items[index].size * (scale / 2.0),
            angle: 0.0,
        };
        let mut start = 0;
        let mut end = 0;
        let pile = matches!(self.container.policy, Policy::Pile { .. });
        let offset = match self.container.policy {
            Policy::Row { tuck_step, .. } => tuck_step,
            _ => Vec2::ZERO,
        };
        let mut parts: Vec<_> = cluster
            .members
            .iter()
            .map(|&index| {
                let edge = self.items[index]
                    .under
                    .map_or(Edge::End, |under| under.edge);
                let (depth, side) = if pile || edge == Edge::End {
                    end += 1;
                    (end, 1.0)
                } else {
                    start += 1;
                    (start, -1.0)
                };
                let centre = match self.container.policy {
                    Policy::Pile { offset } => Pile {
                        step: (offset * scale).extend(0.0),
                    }
                    .offset(depth)
                    .truncate(),
                    _ => offset * Vec2::new(side, 1.0) * (depth as f32 * scale),
                };
                (index, rectangle(index, centre))
            })
            .collect();
        parts.reverse();
        parts.push((cluster.face, rectangle(cluster.face, Vec2::ZERO)));

        let (mut low, mut high) = (Vec2::ZERO, Vec2::ZERO);
        for &(_, rect) in &parts {
            let mut half = rect.half;
            if let Policy::Fan(Fan::Fixed { max_lean, .. }) = self.container.policy {
                // Reserve room for a turned rectangle without depending on the positions or
                // number of the items that follow it.
                half.x += half.y * max_lean.abs().min(std::f32::consts::FRAC_PI_2).sin();
            }
            low = low.min(rect.centre - half);
            high = high.max(rect.centre + half);
        }
        Pack { parts, low, high }
    }

    fn local(&self, mut rect: Rect, centre: Vec2) -> Rect {
        rect.centre += centre;
        if let Policy::Fan(Fan::Fixed {
            radius, max_lean, ..
        }) = self.container.policy
            && radius > 0.0
        {
            let fraction = (rect.centre.x / radius).clamp(-1.0, 1.0);
            rect.centre.y -= radius * (1.0 - (1.0 - fraction * fraction).sqrt());
            rect.angle = -fraction.asin().clamp(-max_lean.abs(), max_lean.abs());
        }
        rect
    }

    fn surface(
        &self,
        index: usize,
        rect: Rect,
        scale: f32,
        depth: f32,
    ) -> Result<Surface, FitError> {
        let region = self.container.region;
        let size = self.items[index].size;
        if !size.is_finite() || size.min_element() <= 0.0 {
            return Err(FitError::Unreachable);
        }
        let points = rect.corners();
        if points.iter().any(|point| {
            !point.is_finite()
                || point
                    .abs()
                    .cmpgt(region.size / 2.0 + Vec2::splat(0.0001))
                    .any()
        }) {
            return Err(FitError::OutsideRegion);
        }
        // Moving each layer along its rays keeps its projection, including the exposed
        // strip, unchanged. At a common point on screen the higher layer is nearer.
        let factor = 1.0 / (1.0 + depth * 0.0001);
        let position = region.point(rect.centre);
        let pose = spot(
            position + (self.camera.eye - position) * (1.0 - factor),
            region.rotation * Quat::from_rotation_z(rect.angle),
            scale * factor,
            Pose::default(),
        );
        let transform = pose.transform();
        let half = size / 2.0;
        let corners = (Rect {
            centre: Vec2::ZERO,
            half,
            angle: 0.0,
        })
        .corners()
        .map(|point| transform.transform_point3(point.extend(0.0)));
        if corners
            .iter()
            .any(|&point| !(0.0..=1.0).contains(&self.camera.project(point).z))
        {
            return Err(FitError::Unreachable);
        }
        let quad = Quad::new(corners.map(|point| self.camera.to_screen(point)));
        (quad.has_area() && quad.bounds.overlaps(self.screen.bounds))
            .then_some(Surface {
                index,
                pose,
                quad,
                press_square: None,
            })
            .ok_or(FitError::Unreachable)
    }

    /// The least portion of this rectangle's width that still holds the target square when
    /// the next rectangle starts at the cut. The strip is measured after camera projection,
    /// so a rectangle at the end of a tilted row can need more room than one near the middle.
    fn least_strip(&self, line: Line, whole: f32, surface: &Surface) -> f32 {
        let target = self.container.limits.press_points;
        if !self.items[surface.index].pressable || target <= 0.0 {
            return 0.0;
        }
        let area = PressArea::new(&surface.quad, &self.screen, target);
        let (mut low, mut high) = (0.0, whole);
        for _ in 0..FIT_STEPS {
            let middle = f32::midpoint(low, high);
            let cut = line.edge + line.direction * middle;
            if area.holds_in(press::strip(
                self.camera,
                self.container.region,
                cut,
                line.direction,
            )) {
                high = middle;
            } else {
                low = middle;
            }
        }
        high
    }

    pub(super) fn fit(
        &self,
        clusters: &[Cluster],
        scale: f32,
        overlap: bool,
    ) -> Result<Vec<Placed>, FitError> {
        let policy = self.container.policy;
        if matches!(policy, Policy::Pile { .. } | Policy::Spot) && clusters.len() > 1 {
            return Err(FitError::OutsideRegion);
        }
        let mut surfaces = Vec::new();
        let mut positioner = Positioner::new(self.container.region, policy, scale);
        for (slot, cluster) in clusters.iter().enumerate() {
            let pack = self.pack(cluster, scale);
            let group = self.items[cluster.face].group.map(|group| group.key);
            let centre = positioner.place(&pack, group);
            for (behind, &(index, rect)) in pack.parts.iter().rev().enumerate().rev() {
                let local = self.local(rect, centre);
                // Reserve a depth interval per cluster. The host keeps its depth when
                // a member behind it leaves.
                let depth = slot as f32 + 1.0 / (1.0 + behind as f32) - 1.0;
                surfaces.push(self.surface(index, local, scale, depth)?);
            }
            if let Some(line) = positioner.line() {
                let whole = pack.reach(line.direction);
                let preferred = match policy {
                    Policy::Cascade { step, .. } if !overlap => step.max(0.0) * scale,
                    _ if overlap => 0.0,
                    _ => whole,
                };
                let step = if preferred < whole && cluster.members.is_empty() {
                    let leading = centre.dot(line.direction) + pack.leading(line.direction);
                    let strip_line = Line {
                        edge: line.edge
                            + line.direction * (leading - line.edge.dot(line.direction)),
                        ..line
                    };
                    let face = surfaces.last().expect("the pack includes its face");
                    preferred.max(self.least_strip(strip_line, whole, face))
                } else {
                    preferred.max(whole)
                };
                positioner.advance(&pack, step, overlap);
            }
        }
        let target = self.container.limits.press_points;
        for index in 0..surfaces.len() {
            let (behind, in_front) = surfaces.split_at_mut(index + 1);
            let face = &mut behind[index];
            if self.items[face.index].pressable && target > 0.0 {
                let blockers = in_front
                    .iter()
                    .map(|other| &other.quad)
                    .filter(|other| other.bounds.overlaps(face.quad.bounds));
                let square = PressArea::new(&face.quad, &self.screen, target)
                    .witness(blockers)
                    .ok_or(FitError::Unreachable)?;
                face.press_square = Some(square);
            }
        }
        Ok(surfaces
            .into_iter()
            .enumerate()
            .map(|(layer, surface)| Placed {
                key: self.items[surface.index].key,
                pose: surface.pose,
                in_view: true,
                count: 1,
                layer,
                press_square: surface.press_square,
            })
            .collect())
    }

    /// Shrink towards the configured floor as the old row zoom did. The press target
    /// can give the usable scales a lower bound, so refine intervals whose ends have
    /// different failure reasons, even when neither end fits.
    pub(super) fn shrink(
        &self,
        clusters: &[Cluster],
        least: f32,
        overlap: bool,
    ) -> Option<(f32, Vec<Placed>)> {
        let mut high = 1.0;
        let mut high_error = self.fit(clusters, high, overlap).err()?;
        for step in 1..=FIT_STEPS {
            let low = if step == FIT_STEPS {
                least
            } else {
                1.0 - (1.0 - least) * (step as f32 / FIT_STEPS as f32)
            };
            let fit = self.fit(clusters, low, overlap);
            let low_error = fit.as_ref().err().copied();
            if low_error != Some(high_error)
                && let Some(placed) = self.refine(clusters, overlap, (low, high), fit, high_error)
            {
                return Some(placed);
            }
            high_error = low_error.unwrap_or(high_error);
            high = low;
        }
        None
    }

    fn refine(
        &self,
        clusters: &[Cluster],
        overlap: bool,
        (mut low, mut high): (f32, f32),
        first: Result<Vec<Placed>, FitError>,
        high_error: FitError,
    ) -> Option<(f32, Vec<Placed>)> {
        let mut best = first.ok().map(|placed| (low, placed));
        for _ in 0..FIT_STEPS {
            let scale = f32::midpoint(low, high);
            match self.fit(clusters, scale, overlap) {
                Ok(placed) => {
                    low = scale;
                    best = Some((scale, placed));
                }
                Err(err) if best.is_some() || err == high_error => high = scale,
                Err(_) => low = scale,
            }
        }
        best
    }

    /// Fill the window from the requested cluster, then fill back towards the start if the
    /// last cluster fits. Each candidate must keep the target square on every visible item.
    pub(super) fn window(
        &self,
        clusters: &[Cluster],
        least: f32,
        overlap: bool,
        scroll: usize,
    ) -> Solved {
        let enabled = self.container.limits.scroll;
        let mut first = if enabled {
            scroll.min(clusters.len() - 1)
        } else {
            0
        };
        let mut scale = least;
        if self.fit(&clusters[first..=first], least, overlap).is_err() {
            // A tiny item can be pressable even when its size at the scale floor is too
            // small. Use a larger scale rather than hiding the first cluster for that reason.
            if self.fit(&clusters[first..=first], 1.0, overlap).is_ok() {
                scale = 1.0;
            } else if let Some((larger, _)) = self.shrink(&clusters[first..=first], least, overlap)
            {
                scale = larger;
            }
        }
        let end = greatest(clusters.len() - first, |count| {
            self.fit(&clusters[first..first + count], scale, overlap)
                .is_ok()
        }) + first;
        if enabled && end == clusters.len() {
            // A window starts at a fixed origin. Prepending a large cluster may fail even
            // when prepending two would fit (for example with a changed fan angle).
            while first > 0 && self.fit(&clusters[first - 1..end], scale, overlap).is_ok() {
                first -= 1;
            }
        }
        let placed = self
            .fit(&clusters[first..end], scale, overlap)
            .expect("the window was checked before it was selected");
        let stage = if enabled {
            Overflow::Scrolled
        } else {
            Overflow::Clipped
        };
        self.solved(clusters, (first, end), placed, stage)
    }

    pub(super) fn solved(
        &self,
        clusters: &[Cluster],
        (first, end): (usize, usize),
        placed: Vec<Placed>,
        overflow: Overflow,
    ) -> Solved {
        let visible: BTreeMap<_, _> = placed.into_iter().map(|item| (item.key, item)).collect();
        let rest = Pose {
            position: self.container.region.centre,
            rotation: self.container.region.rotation,
            ..Pose::default()
        };
        let edge = |cluster: Option<&Cluster>| {
            cluster
                .and_then(|cluster| visible.get(&self.items[cluster.face].key))
                .map_or(rest, |item| item.pose)
        };
        let before_pose = edge(clusters.get(first));
        let after_pose = edge(end.checked_sub(1).and_then(|i| clusters.get(i)));
        let hidden = |index: usize, pose, count| Placed {
            key: self.items[index].key,
            pose,
            in_view: false,
            count,
            layer: 0,
            press_square: None,
        };
        let mut results: Vec<_> = (0..self.items.len()).map(|i| hidden(i, rest, 0)).collect();
        let counted_pile = matches!(self.container.policy, Policy::Pile { .. });
        for (index, cluster) in clusters.iter().enumerate() {
            let edge = if index < first {
                before_pose
            } else {
                after_pose
            };
            let get = |item: usize, count| {
                visible.get(&self.items[item].key).map_or_else(
                    || hidden(item, edge, count),
                    |&placed| Placed { count, ..placed },
                )
            };
            let count =
                1 + cluster.copies.len() + usize::from(counted_pile) * cluster.members.len();
            let face = get(cluster.face, count);
            results[cluster.face] = face;
            for &member in &cluster.members {
                results[member] = get(member, usize::from(!counted_pile));
            }
            for &copy in &cluster.copies {
                results[copy] = hidden(copy, face.pose, 0);
            }
        }
        let scroll = (overflow == Overflow::Scrolled).then(|| {
            let last = if end == clusters.len() {
                first
            } else {
                clusters.len() - 1
            };
            let axis = match self.container.policy {
                Policy::Grid { .. } => ScrollAxis::Vertical,
                Policy::Cascade { direction, .. } if direction.y.abs() > direction.x.abs() => {
                    ScrollAxis::Vertical
                }
                _ => ScrollAxis::Horizontal,
            };
            GroupWindow {
                first,
                shown: end - first,
                last,
                axis,
            }
        });
        Solved {
            items: results,
            scroll,
            overflow,
            before: clusters[..first].iter().map(Cluster::len).sum(),
            after: clusters[end..].iter().map(Cluster::len).sum(),
        }
    }
}

/// Find the largest fitting prefix. A later cluster cannot change a prefix's positions
/// and can only add occlusion, so failing prefixes stay failing as they grow.
fn greatest(maximum: usize, fits: impl Fn(usize) -> bool) -> usize {
    let (mut low, mut high) = (0, maximum + 1);
    while high - low > 1 {
        let middle = usize::midpoint(low, high);
        if fits(middle) {
            low = middle;
        } else {
            high = middle;
        }
    }
    low
}

struct Positioner {
    policy: Policy,
    scale: f32,
    lines: [Line; 2],
    previous_groups: [Option<u64>; 2],
    index: usize,
    grid: Grid,
}

impl Positioner {
    fn new(region: super::Region, policy: Policy, scale: f32) -> Self {
        let half = region.size / 2.0;
        let (direction, alignment) = match policy {
            Policy::Row { alignment, .. } | Policy::Fan(Fan::Fixed { alignment, .. }) => {
                (Vec2::X, alignment)
            }
            Policy::Cascade { direction, .. } => (
                direction.try_normalize().unwrap_or(Vec2::Y),
                Alignment::Start,
            ),
            _ => (Vec2::X, Alignment::Start),
        };
        let first = match alignment {
            Alignment::Start => Line::start(half, direction),
            Alignment::End => Line::start(half, -direction),
            Alignment::Middle => Line {
                edge: Vec2::ZERO,
                direction,
            },
        };
        Self {
            policy,
            scale,
            lines: [
                first,
                Line {
                    edge: Vec2::ZERO,
                    direction: -direction,
                },
            ],
            previous_groups: [None; 2],
            index: 0,
            grid: Grid {
                left: -half.x,
                right: half.x,
                x: -half.x,
                y: half.y,
                height: 0.0,
                column: 0,
            },
        }
    }

    fn centred(&self) -> bool {
        matches!(
            self.policy,
            Policy::Row {
                alignment: Alignment::Middle,
                ..
            } | Policy::Fan(Fan::Fixed {
                alignment: Alignment::Middle,
                ..
            })
        )
    }

    fn active(&self) -> usize {
        if self.centred() {
            self.index.saturating_sub(1) % 2
        } else {
            0
        }
    }

    fn line(&self) -> Option<Line> {
        matches!(
            self.policy,
            Policy::Row { .. } | Policy::Fan(_) | Policy::Cascade { .. }
        )
        .then(|| self.lines[self.active()])
    }

    fn place(&mut self, pack: &Pack, group: Option<u64>) -> Vec2 {
        match self.policy {
            Policy::Grid { columns, gap } => {
                self.grid
                    .place(pack, columns, gap.max(Vec2::ZERO) * self.scale)
            }
            Policy::Spot | Policy::Pile { .. } => Vec2::ZERO,
            _ if self.centred() && self.index == 0 => {
                self.previous_groups = [group; 2];
                Vec2::ZERO
            }
            _ => {
                let active = self.active();
                let group_gap = match self.policy {
                    Policy::Row { group_gap, .. }
                        if self.index > 0 && self.previous_groups[active] != group =>
                    {
                        group_gap.max(0.0) * self.scale
                    }
                    _ => 0.0,
                };
                self.previous_groups[active] = group;
                let line = &mut self.lines[active];
                line.advance(group_gap);
                line.centre(pack)
            }
        }
    }

    fn advance(&mut self, pack: &Pack, step: f32, overlap: bool) {
        let gap = match self.policy {
            Policy::Row { gap, .. } | Policy::Fan(Fan::Fixed { gap, .. }) if !overlap => {
                gap.max(0.0) * self.scale
            }
            _ => 0.0,
        };
        if self.centred() && self.index == 0 {
            self.lines[0].edge.x = pack.high.x + gap;
            self.lines[1].edge.x = pack.low.x - gap;
        } else {
            let active = self.active();
            self.lines[active].advance(step + gap);
        }
        self.index += 1;
    }
}

struct Grid {
    left: f32,
    right: f32,
    x: f32,
    y: f32,
    height: f32,
    column: usize,
}

impl Grid {
    fn place(&mut self, pack: &Pack, columns: usize, gap: Vec2) -> Vec2 {
        let size = pack.size();
        if self.column > 0 && (self.column >= columns.max(1) || self.x + size.x > self.right) {
            self.x = self.left;
            self.y -= self.height + gap.y;
            self.height = 0.0;
            self.column = 0;
        }
        let centre = Vec2::new(self.x - pack.low.x, self.y - pack.high.y);
        self.x += size.x + gap.x;
        self.height = self.height.max(size.y);
        self.column += 1;
        centre
    }
}
