//! Vector artwork descriptions sent to the atlas worker.

use glam::Vec2;

use super::{IconCell, IconId};

/// One icon's artwork.
#[derive(Clone, Copy, Debug)]
pub struct Artwork {
    pub icon: IconId,
    /// The side of the square box the web draws it in, in logical points.
    pub size: u16,
    /// The SVG viewbox: its left, top, width and height. The viewbox is fitted into the square
    /// cell and centred in it, as SVG's default `xMidYMid meet` does.
    pub view: [f32; 4],
    /// The shapes, painted in order.
    pub shapes: &'static [Shape],
}

/// One `<path>` of an icon.
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    /// Its `d` attribute.
    pub data: &'static str,
    pub fill: Option<Fill>,
    pub stroke: Option<Outline>,
    /// A `rotate(degrees cx cy)` transform, in viewbox units.
    pub rotate: Option<[f32; 3]>,
}

/// How a shape is filled.
#[derive(Clone, Copy, Debug)]
pub struct Fill {
    /// The fill's brightness against the icon's main color, from 0 to 1.
    pub ink: f32,
    /// SVG's `fill-rule: evenodd`, where nonzero is the default.
    pub even_odd: bool,
}

/// How a shape is stroked: always with round caps and joins, as every stroked icon of the web
/// interface is (an open path's caps are what the eye sees; a closed one has none).
#[derive(Clone, Copy, Debug)]
pub struct Outline {
    /// The stroke's width, in viewbox units.
    pub width: f32,
}

impl Artwork {
    /// The side of the square cell at `scale` physical pixels per point.
    #[must_use]
    pub fn side(self, scale: f32) -> u16 {
        texels(f32::from(self.size) * scale)
    }

    /// Records the icon or one of its shadows at its rectangle on an atlas page.
    #[must_use]
    pub fn cell(self, rect: [u16; 4], shadow: u8) -> IconCell {
        IconCell {
            icon: self.icon,
            size: self.size,
            rect,
            shadow,
        }
    }
}

/// A layer's atlas key and the artwork rectangle the texture covers.
#[derive(Clone, Copy, Debug)]
pub struct LayerCell {
    pub icon: IconId,
    /// Left, top, width, and height in artwork units.
    pub view: [f32; 4],
    /// Width in logical points, rounded to the atlas key.
    pub size: u16,
    /// Maximum raster scale for art that can share the same resolution across displays.
    pub max_scale: Option<f32>,
}

impl LayerCell {
    /// Texture dimensions at the display scale, preserving the artwork's proportions.
    #[must_use]
    pub fn texels(self, scale: f32) -> [u16; 2] {
        let [_, _, width, height] = self.view;
        let scale = self.max_scale.map_or(scale, |limit| scale.min(limit));
        let scaled_width = f32::from(self.size) * scale;
        [texels(scaled_width), texels(scaled_width * height / width)]
    }

    /// Records a layer at its rectangle on an atlas page.
    #[must_use]
    pub fn cell(self, rect: [u16; 4]) -> IconCell {
        IconCell {
            icon: self.icon,
            size: self.size,
            rect,
            shadow: 0,
        }
    }
}

/// The artwork and effects rasterized into one single-channel cell of a layered icon.
#[derive(Debug)]
pub struct CoverageLayer {
    pub cell: LayerCell,
    /// In artwork units, with a flag that closes each outline.
    pub outlines: Vec<(Vec<Vec2>, bool)>,
    pub paint: CoveragePaint,
    pub ramp: CoverageRamp,
    /// Blur deviation and downward offset, both in artwork units.
    pub blur: f32,
    pub drop: f32,
    /// The opacity of the blur beneath the original coverage. With none, keep only the blur.
    pub glow_opacity: Option<f32>,
}

/// How the outlines of a layer are painted into coverage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoveragePaint {
    pub fill: bool,
    /// Width in artwork units and whether the stroke has round caps.
    pub stroke: Option<(f32, bool)>,
}

/// How the coverage of a layer fades across the artwork.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CoverageRamp {
    None,
    /// The normalized positions where coverage starts, reaches full, starts fading, and ends.
    Vertical {
        top: f32,
        height: f32,
        stops: [f32; 4],
    },
    /// Coverage is full within `whole` units of the origin, and transparent past `gone`.
    Radial {
        whole: f32,
        gone: f32,
    },
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to one atlas page"
)]
pub(super) fn texels(length: f32) -> u16 {
    length.round().clamp(1.0, 1024.0) as u16
}
