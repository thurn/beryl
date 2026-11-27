//! The spinner under the opponent's orb while the AI decides: a faint ring with a dim arc and a
//! bright arc on it. The arcs turn together through `IconLayer::rotation`, so the picture is
//! rasterized once and only the angle changes from frame to frame.

use std::f32::consts::TAU;

use glam::Vec2;
use kit_base::atlas::IconId;
use kit_base::atlas::artwork::{CoverageLayer, CoveragePaint, CoverageRamp, LayerCell};
use kit_base::tokens::{color, size};
use kit_ui::element::Shadow;
use kit_ui::widgets::icons::{IconLayer, LayeredIcon};

/// The side of the spinner's box in logical points, which is also its side in artwork units.
pub const SIZE: f32 = size::THINKING;
/// The ring's and the arcs' width in artwork units.
const TRACK_WIDTH: f32 = size::THINKING_TRACK;
const ARC_WIDTH: f32 = 3.4;
/// The radius of the ring, leaving room for the arcs' round caps inside the box.
const RADIUS: f32 = SIZE / 2.0 - ARC_WIDTH;

/// One cell of the spinner: the part of the circle it covers, in turns clockwise from the top, and how it is
/// painted.
struct Part {
    from: f32,
    to: f32,
    width: f32,
    opacity: u8,
    rotation: f32,
    tint: [u8; 4],
}

const PARTS: [Part; 3] = [
    // The track: the whole ring, still, in a pale cream at about a fifth of full strength.
    Part {
        from: 0.0,
        to: 1.0,
        width: TRACK_WIDTH,
        opacity: 56,
        rotation: 0.0,
        tint: [255, 214, 150, 255],
    },
    // The tail: half a turn at about two fifths of full strength.
    Part {
        from: 0.0,
        to: 0.5,
        width: ARC_WIDTH,
        opacity: 110,
        rotation: 360.0,
        tint: color::THINKING,
    },
    // The head: just under a third of a turn, full strength, where the tail ends.
    Part {
        from: 0.5,
        to: 0.8,
        width: ARC_WIDTH,
        opacity: 255,
        rotation: 360.0,
        tint: [255, 243, 192, 255],
    },
];

const fn cell(index: usize) -> LayerCell {
    LayerCell {
        icon: IconId(super::orb::END_ICON + super::turn::ICONS + index as u16),
        view: [-SIZE / 2.0, -SIZE / 2.0, SIZE, SIZE],
        size: SIZE as u16,
        max_scale: None,
    }
}

pub(super) fn atlas_cells() -> impl Iterator<Item = LayerCell> {
    (0..PARTS.len()).map(cell)
}

pub(super) fn atlas_artwork() -> impl Iterator<Item = CoverageLayer> {
    PARTS.iter().enumerate().map(|(index, part)| {
        let closed = part.to - part.from >= 1.0;
        let steps = (96.0 * (part.to - part.from)).ceil() as u16;
        let outline = (0..=steps)
            .take(if closed {
                usize::from(steps)
            } else {
                usize::MAX
            })
            .map(|step| {
                let turns = part.from + (part.to - part.from) * f32::from(step) / f32::from(steps);
                // Turns count clockwise from the top, and the screen's y points down.
                Vec2::new((TAU * turns).sin(), -(TAU * turns).cos()) * RADIUS
            })
            .collect();
        CoverageLayer {
            cell: cell(index),
            outlines: vec![(outline, closed)],
            paint: CoveragePaint {
                fill: false,
                stroke: Some((part.width, !closed)),
            },
            ramp: CoverageRamp::None,
            blur: Shadow::new(0.0, 0.0, 3.0, [0; 4]).drop_sigma(),
            drop: 0.0,
            glow_opacity: Some(0.5),
        }
    })
}

const fn layers() -> [IconLayer; PARTS.len()] {
    let mut layers = [IconLayer {
        icon: IconId(0),
        view: [0.0; 4],
        color: [0; 4],
        highlight: false,
        rotation: 0.0,
    }; PARTS.len()];
    let mut index = 0;
    while index < PARTS.len() {
        let part = &PARTS[index];
        let mut tint = part.tint;
        tint[3] = part.opacity;
        layers[index] = IconLayer {
            icon: cell(index).icon,
            view: cell(index).view,
            color: tint,
            highlight: false,
            rotation: part.rotation,
        };
        index += 1;
    }
    layers
}

/// The spinner's layers. Over `icon_progress` 0 to 1 the arcs make one turn.
pub const fn artwork() -> LayeredIcon {
    const LAYERS: [IconLayer; PARTS.len()] = layers();
    LayeredIcon {
        layers: &LAYERS,
        origin: [-SIZE / 2.0; 2],
        scale: 1.0,
        highlighted: false,
    }
}
