//! The turn banner's circles in drawing coordinates. Each set of circles
//! shares a single coverage cell so its crossings stay the same opacity as the rest of it.

use std::f32::consts::TAU;

use beryl_protocol::protocol::Side;
use glam::Vec2;
use kit_base::atlas::IconId;
use kit_base::atlas::artwork::{CoverageLayer, CoveragePaint, CoverageRamp, LayerCell};
use kit_base::tokens::{color, size};
use kit_ui::element::Shadow;
use kit_ui::widgets::icons::{IconLayer, LayeredIcon};

/// The web's 220-unit view box drawn at 240 points.
pub(crate) const SIZE: f32 = size::TURN_BANNER_RING_SIZE;
const UNITS: f32 = SIZE / 220.0;
const SWEEP: f32 = 80.0;

struct Circles {
    radius: f32,
    count: u16,
    offset: f32,
    stroke: f32,
    side: u16,
    opacity: u8,
    rotation: f32,
}

const OUTER: Circles = Circles {
    radius: 50.0,
    count: 12,
    offset: 0.0,
    stroke: 0.7,
    side: 240,
    opacity: 140,
    rotation: SWEEP,
};
const INNER: Circles = Circles {
    radius: 32.0,
    count: 8,
    offset: 0.5,
    stroke: 0.9,
    side: 164,
    opacity: 102,
    rotation: -SWEEP,
};
const RIMS: Circles = Circles {
    radius: 101.0,
    count: 2,
    offset: 0.0,
    stroke: 0.6,
    side: 248,
    opacity: 128,
    rotation: 0.0,
};

const GROUPS: [Circles; 3] = [OUTER, INNER, RIMS];

pub(crate) const fn tint(side: Side) -> [u8; 4] {
    match side {
        Side::You => color::CYAN,
        Side::Opponent => color::EMBER,
    }
}

const fn cell(group: &Circles, index: usize) -> LayerCell {
    let side = group.side as f32;
    LayerCell {
        icon: IconId(super::orb::END_ICON + index as u16),
        view: [-side / 2.0, -side / 2.0, side, side],
        size: group.side,
        // The banner is translucent and the compact layout draws it at 45% size. One texel
        // per artwork point keeps the circles visible while leaving atlas pages for glyphs.
        max_scale: Some(1.0),
    }
}

pub(super) fn atlas_cells() -> impl Iterator<Item = LayerCell> {
    GROUPS
        .iter()
        .enumerate()
        .map(|(index, group)| cell(group, index))
}

pub(super) fn atlas_artwork() -> impl Iterator<Item = CoverageLayer> {
    GROUPS
        .iter()
        .zip(atlas_cells())
        .map(|(group, cell)| CoverageLayer {
            cell,
            outlines: (0..group.count)
                .map(|index| {
                    let (center, radius) = if group.rotation == 0.0 {
                        (Vec2::ZERO, group.radius + 3.0 * f32::from(index))
                    } else {
                        let angle =
                            TAU * (f32::from(index) + group.offset) / f32::from(group.count);
                        (Vec2::from_angle(angle) * group.radius, group.radius)
                    };
                    let outline = (0_u16..192)
                        .map(|step| {
                            (center + Vec2::from_angle(TAU * f32::from(step) / 192.0) * radius)
                                * UNITS
                        })
                        .collect();
                    (outline, true)
                })
                .collect(),
            paint: CoveragePaint {
                fill: false,
                stroke: Some((group.stroke * UNITS, false)),
            },
            ramp: CoverageRamp::None,
            blur: Shadow::new(0.0, 0.0, 4.0, [0; 4]).drop_sigma(),
            drop: 0.0,
            glow_opacity: Some(0.55),
        })
}

const fn layers(side: Side) -> [IconLayer; GROUPS.len()] {
    let mut layers = [IconLayer {
        icon: IconId(0),
        view: [0.0; 4],
        color: [0; 4],
        highlight: false,
        rotation: 0.0,
    }; GROUPS.len()];
    let mut index = 0;
    while index < GROUPS.len() {
        let group = &GROUPS[index];
        let cell = cell(group, index);
        let mut color = tint(side);
        color[3] = group.opacity;
        layers[index] = IconLayer {
            icon: cell.icon,
            view: cell.view,
            color,
            highlight: false,
            rotation: group.rotation,
        };
        index += 1;
    }
    layers
}

pub(crate) const fn artwork(side: Side) -> LayeredIcon {
    const YOU: [IconLayer; GROUPS.len()] = layers(Side::You);
    const OPPONENT: [IconLayer; GROUPS.len()] = layers(Side::Opponent);
    LayeredIcon {
        layers: match side {
            Side::You => &YOU,
            Side::Opponent => &OPPONENT,
        },
        origin: [-SIZE / 2.0; 2],
        scale: 1.0,
        highlighted: false,
    }
}
