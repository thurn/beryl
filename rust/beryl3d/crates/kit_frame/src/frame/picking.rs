//! Point queries over scene nodes, including layout between two finished frames.

use std::cmp::Reverse;

use glam::Vec2;
use kit_base::geometry::Rect;

use super::{Band, Frame, PaintBand, Resolved, order::ordered};

impl Frame {
    /// The frontmost eligible element at `point` in logical points, before any input
    /// redirection such as a shown card's screen host.
    pub fn pick(&self, point: Vec2, accepts: impl Fn(&Resolved) -> bool) -> Option<u32> {
        self.pick_from(point, self.paint_order().rev(), accepts)
    }

    /// Picks from a subset of nodes in front-to-back paint order, as used by coverage checks.
    pub fn pick_from(
        &self,
        point: Vec2,
        front_first: impl Iterator<Item = u32> + Clone,
        accepts: impl Fn(&Resolved) -> bool,
    ) -> Option<u32> {
        let node = |index: u32| &self.resolved[index as usize];
        let accepts = &accepts;
        let covers = |index| {
            let element = node(index);
            accepts(element) && Area::Quad(element.corners, element.clip).contains(point)
        };
        let mut hits = front_first.filter(|&index| covers(index));
        let first = hits.next()?;
        // Flat layers and input regions, such as a pile's hit box, keep scene order.
        let Some(first_band) = self.depth_card_band(first) else {
            return Some(first);
        };
        let band = PaintBand::of(node(first));
        let others = hits
            .take_while(|&index| PaintBand::of(node(index)) == band)
            .filter_map(|index| Some((index, self.depth_card_band(index)?)));
        let mut others = others.peekable();
        if others.peek().is_none() {
            return Some(first);
        }
        let cards = std::iter::once((first, first_band)).chain(others);
        let opaque = |band| matches!(band, Band::Opaque | Band::MovingOpaque);
        // Opaque cards write depth with Less. The closest one wins, with ties going to
        // the card submitted first (nearer origin, then later registration).
        let written = cards
            .clone()
            .filter(|&(_, band)| opaque(band))
            .map(|(index, _)| {
                let (depth, center) = self.card_depths(node(index), point, band);
                (ordered(depth), ordered(center), Reverse(index))
            })
            .min();
        // Blended cards draw later with LessEqual and leave depth unchanged. The last card
        // passing the opaque depth test paints over the others, even at the same depth.
        let blended = cards
            .filter(|&(_, band)| !opaque(band))
            .filter_map(|(index, pass)| {
                let (depth, center) = self.card_depths(node(index), point, band);
                written
                    .is_none_or(|(limit, ..)| ordered(depth) <= limit)
                    .then_some((pass, Reverse(ordered(center)), index))
            })
            .max();
        Some(blended.map_or_else(
            || written.map_or(first, |(.., Reverse(index))| index),
            |(.., index)| index,
        ))
    }

    /// Whether `a` could be picked ahead of `b` somewhere in their overlap. Cards in the
    /// same depth-tested scene band can cross, so their center order cannot rule one out.
    #[must_use]
    pub fn may_paint_over(&self, a: u32, b: u32) -> bool {
        self.compare_paint(a, b).is_gt()
            || (a != b
                && self.depth_card_band(a).is_some()
                && self.depth_card_band(b).is_some()
                && PaintBand::of(&self.resolved[a as usize])
                    == PaintBand::of(&self.resolved[b as usize]))
    }

    /// The depth buffer value at the pointer and the projected depth of the card's origin.
    fn card_depths(&self, element: &Resolved, point: Vec2, band: PaintBand) -> (f32, f32) {
        let pose = element.pose.expect("a drawn card has a pose");
        let transform = pose.transform();
        let half = self.card * 0.5;
        let center = self.view_projection.project_point3(pose.position).z;
        let [corner, across, _, up] = element.corners;
        let (across, up, offset) = (across - corner, up - corner, point - corner);
        let area = across.perp_dot(up);
        // After the perspective divide, depth varies linearly across the screen. Use three
        // corners of the card's plane to interpolate it at the hit point.
        let depth = if area.abs() > f32::EPSILON {
            let z = |x, y| {
                self.view_projection
                    .project_point3(transform.transform_point3(glam::Vec3::new(x, y, 0.0)))
                    .z
            };
            let base = z(-half.x, -half.y);
            let horizontal = offset.perp_dot(up) / area;
            let vertical = across.perp_dot(offset) / area;
            base + horizontal * (z(half.x, -half.y) - base) + vertical * (z(-half.x, half.y) - base)
        } else {
            center
        };
        // The renderer gives the moving and resting passes separate halves of the viewport
        // depth range. Round in that range before comparing two samples.
        let minimum = if band == PaintBand::Moving { 0.0 } else { 0.5 };
        (minimum + depth * 0.5, center)
    }
}

/// A projected or laid-out region and the clip that limits it, in logical points.
#[derive(Clone, Copy)]
pub enum Area {
    Quad([Vec2; 4], Option<Rect>),
    Rect(Rect, Option<Rect>),
}

/// Finds the frontmost candidate at `point` that `accept` wants. The visitor supplies its
/// candidates in reverse paint order and stops at the first successful test.
pub fn hit_test<T, C>(
    point: Vec2,
    visit: impl FnOnce(&dyn Fn(Area, C) -> Option<T>) -> Option<T>,
    accept: impl Fn(C) -> Option<T>,
) -> Option<T> {
    visit(&|area, candidate| area.contains(point).then(|| accept(candidate)).flatten())
}

impl Area {
    /// Whether the point lies within both the shape and its clip.
    #[must_use]
    pub fn contains(self, point: Vec2) -> bool {
        match self {
            Self::Quad(corners, clip) => {
                clip.is_none_or(|clip| contains(clip, point)) && in_quad(corners, point)
            }
            Self::Rect(rect, clip) => {
                contains(rect, point) && clip.is_none_or(|clip| contains(clip, point))
            }
        }
    }

    /// Intersects a clipping node's box with the clip it inherited.
    #[must_use]
    pub fn clip(rect: Rect, inherited: Option<Rect>) -> Rect {
        let Some(clip) = inherited else {
            return rect;
        };
        let x = rect.x.max(clip.x);
        let y = rect.y.max(clip.y);
        Rect {
            x,
            y,
            width: (rect.x + rect.width).min(clip.x + clip.width) - x,
            height: (rect.y + rect.height).min(clip.y + clip.height) - y,
        }
    }
}

fn contains(rect: Rect, point: Vec2) -> bool {
    rect.width > 0.0
        && rect.height > 0.0
        && point.x >= rect.x
        && point.y >= rect.y
        && point.x <= rect.x + rect.width
        && point.y <= rect.y + rect.height
}

/// Tests a convex quadrilateral in either winding.
fn in_quad(corners: [Vec2; 4], point: Vec2) -> bool {
    let mut side = None;
    for index in 0..4 {
        let a = corners[index];
        let b = corners[(index + 1) % 4];
        let cross = (b - a).perp_dot(point - a);
        if cross.abs() <= f32::EPSILON {
            continue;
        }
        let left = cross > 0.0;
        if *side.get_or_insert(left) != left {
            return false;
        }
    }
    side.is_some()
}
