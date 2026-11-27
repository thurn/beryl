//! Checks that no mark or badge of a covered card is drawn over a card in front of it.
//!
//! Cards on the table and in motion write the depth buffer, and their marks (power/toughness
//! plaques, counter discs, summoning-sickness badges, outlines, notes) are depth-tested against
//! it; cards in `Band::LayerCards` and `Band::Dragged` paint in draw-list order without a depth
//! buffer. If a back card's mark is lifted past a covering front card or placed in a later pass,
//! it shows through the front card.

use std::collections::BTreeSet;

use glam::{Vec2, Vec3};

use super::checks::{fault, subject};
use super::fault::{Context, Fault, Rule};
use kit_frame::frame::picking::Area;
use kit_frame::frame::{Band, Draw, Frame, Material, PaintBand, Resolved, Role};

/// Minimum NDC depth gap for one card to count as strictly in front of another at a screen point.
const DEPTH_EPS: f32 = 1e-6;

/// Checks that no mark of a covered card is drawn over an opaque card in front of it.
/// Requires `Frame::owners` (populated when `Frame::looking` is enabled).
#[must_use]
pub fn card_marks(frame: &Frame, context: &Context) -> Vec<Fault> {
    if frame.owners.len() != frame.draws.len() {
        return Vec::new();
    }
    let cards: Vec<u32> = (0..frame.resolved.len() as u32)
        .filter(|&index| is_card(&frame.resolved[index as usize]))
        .collect();
    if cards.len() < 2 {
        return Vec::new();
    }
    // Map each card to its latest card-face draw index for non-depth-buffered layer bands.
    let mut card_draw = vec![None; frame.resolved.len()];
    for (draw_idx, (draw, &owner)) in frame.draws.iter().zip(&frame.owners).enumerate() {
        if owner != u32::MAX
            && draw.material == Material::Card as u8
            && !draw.is_edge_only()
            && is_card(&frame.resolved[owner as usize])
        {
            card_draw[owner as usize] = Some(draw_idx);
        }
    }

    let mut pairs = BTreeSet::new();
    let mut faults = Vec::new();
    for (draw_idx, (draw, &owner)) in frame.draws.iter().zip(&frame.owners).enumerate() {
        if owner == u32::MAX || !is_card_mark(draw) {
            continue;
        }
        let back = &frame.resolved[owner as usize];
        if !is_card(back) || back.opacity <= context.audit.transparent {
            continue;
        }
        let Some(draw_band) = frame.band(draw_idx) else {
            continue;
        };
        let Some((screen, z_mark)) = project_origin(frame, draw, context) else {
            continue;
        };
        let back_pb = PaintBand::of(back);
        let z_back = plane_ndc_depth(frame, back, screen);

        for &front_idx in &cards {
            if front_idx == owner || pairs.contains(&(owner, front_idx)) {
                continue;
            }
            let front = &frame.resolved[front_idx as usize];
            if front.opacity < 1.0 - 1e-4 {
                continue;
            }
            if !Area::Quad(front.corners, front.clip).contains(screen) {
                continue;
            }
            let front_pb = PaintBand::of(front);
            let z_front = plane_ndc_depth(frame, front, screen);
            let front_covers_back =
                front_pb > back_pb || (front_pb == back_pb && z_front < z_back - DEPTH_EPS);
            if !front_covers_back {
                continue;
            }
            if drawn_over_front(
                draw_band,
                draw_idx,
                z_mark,
                front_pb,
                z_front,
                card_draw[front_idx as usize],
            ) {
                pairs.insert((owner, front_idx));
                let a = subject(frame, owner);
                let b = subject(frame, front_idx);
                faults.push(fault(
                    Rule::Overlap,
                    context,
                    format!("mark of {} is drawn over {}", a.describe(), b.describe()),
                    vec![a, b],
                ));
            }
        }
    }
    faults
}

fn is_card(element: &Resolved) -> bool {
    element.pose.is_some() && matches!(element.role, Role::Card | Role::Custom("ability"))
}

fn is_card_mark(draw: &Draw) -> bool {
    if draw.tint[3] == 0 || draw.is_edge_only() {
        return false;
    }
    matches!(
        draw.material,
        m if m == Material::Panel as u8
            || m == Material::Glyph as u8
            || m == Material::Icon as u8
            || m == Material::BoxShadow as u8
            || m == Material::Glow as u8
            || m == Material::Gradient as u8
    )
}

fn project_origin(frame: &Frame, draw: &Draw, context: &Context) -> Option<(Vec2, f32)> {
    let m = &draw.model;
    let origin = Vec3::new(m[3], m[7], m[11]);
    let clip = frame.view_projection * origin.extend(1.0);
    if clip.w <= f32::EPSILON {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    let screen = Vec2::new(
        f32::midpoint(ndc.x, 1.0) * context.size.width,
        (1.0 - ndc.y) * 0.5 * context.size.height,
    );
    Some((screen, ndc.z))
}

fn plane_ndc_depth(frame: &Frame, element: &Resolved, point: Vec2) -> f32 {
    let pose = element.pose.expect("a drawn card has a pose");
    let transform = pose.transform();
    let half = frame.card * 0.5;
    let [corner, across, _, up] = element.corners;
    let (across, up, offset) = (across - corner, up - corner, point - corner);
    let area = across.perp_dot(up);
    if area.abs() > f32::EPSILON {
        let z = |x, y| {
            frame
                .view_projection
                .project_point3(transform.transform_point3(Vec3::new(x, y, 0.0)))
                .z
        };
        let base = z(-half.x, -half.y);
        let horizontal = offset.perp_dot(up) / area;
        let vertical = across.perp_dot(offset) / area;
        base + horizontal * (z(half.x, -half.y) - base) + vertical * (z(-half.x, half.y) - base)
    } else {
        frame.view_projection.project_point3(pose.position).z
    }
}

fn drawn_over_front(
    draw_band: Band,
    draw_idx: usize,
    z_mark: f32,
    front_pb: PaintBand,
    z_front: f32,
    front_draw_idx: Option<usize>,
) -> bool {
    let front_band = front_pb.records(Band::Opaque);
    match front_band {
        Band::Opaque => {
            if draw_band >= Band::MovingOpaque {
                true
            } else if draw_band > Band::Opaque {
                z_mark <= z_front
            } else {
                false
            }
        }
        Band::MovingOpaque => {
            if draw_band >= Band::Layer {
                true
            } else if draw_band == Band::Moving {
                z_mark <= z_front
            } else {
                false
            }
        }
        _ => match draw_band.cmp(&front_band) {
            std::cmp::Ordering::Greater => true,
            std::cmp::Ordering::Equal => {
                front_draw_idx.is_some_and(|front_idx| draw_idx > front_idx)
            }
            std::cmp::Ordering::Less => false,
        },
    }
}
