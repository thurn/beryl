//! The art of a life orb: the web interface's `LifeOrb` (`ui/src/components/Player.tsx`, styled
//! by `ui/src/design/components/life-orbs.css`), drawn in the same units, the mockup pixels of
//! its 272 by 116 view box about the orb's middle.
//!
//! - The core is a cushion, a superellipse `CORE_RX` by `CORE_RY` about (0, `CORE_CY`), filled
//!   dark inside a bezel `BEZEL` wide, light along its top and dark along its foot, over a soft
//!   drop shadow.
//! - A gold ring underlines the core just outside its bezel, toward its player's edge of the
//!   table, over a blurred orange glow.
//! - The active player's orb flares: an orange glow and eight rays fan out from behind the core
//!   away from their edge, a blurred arc of light rims the core on that side, and inside the
//!   core a crescent and a bloom of light lie along the same rim.
//! - The opponent's orb is the viewer's mirrored top to bottom, still lit from the top of the
//!   screen: its bezel is light along its top and its shadow falls below it.
//!
//! The flat layer's atlas holds one channel, coverage, so the art is a stack of layers, each one
//! color with its coverage drawn by the glyph worker into an icon cell, painted in order. A
//! gradient becomes coverage that fades along it: the bezel's four colors are four layers, each
//! opaque where the next one up starts and fading out where it ends, so that each pixel shows
//! the blend of the two colors its gradient runs between. A blur is drawn into the coverage too.
//! The game screen draws an orb as one element showing `Shown::Layers`, whose layers the flat layer
//! draws from these cells (`layout::write`).

use beryl_protocol::protocol::Side;
use glam::Vec2;

use game_play::art::orb::CORE_CY;
pub use game_play::art::orb::{ART, face_offset};
use kit_base::atlas::IconId;
use kit_base::atlas::artwork::{CoverageLayer, CoveragePaint, CoverageRamp, LayerCell};
use kit_base::tokens::size;
use kit_ui::art::icons as icon_artwork;
use kit_ui::widgets::icons::{IconLayer, LayeredIcon, cell_width};

/// The core's half width and half height.
pub const CORE_RX: f32 = size::ORB_WIDTH / 2.0;
pub const CORE_RY: f32 = size::ORB_HEIGHT / 2.0;

/// The superellipse's exponent: `SQUARENESS`.
const SQUARENESS: f32 = 2.25;

/// The width of the core's bezel: `BEZEL`, the stroke of `.life-orb-core`.
pub const BEZEL: f32 = size::ORB_BEZEL;

pub use game_table::profiles::ORB_SCALES as SCALES;

/// One outline of the art, in units, as the viewer's orb draws it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// The core's whole outline, closed.
    Core,
    /// The part of the core's outline from `from` to `to` degrees, clockwise from its right,
    /// moved `offset` units out along its normal (in, if negative): `rim` in `Player.tsx`.
    Rim { offset: f32, from: f32, to: f32 },
    /// The part of an ellipse from `from` to `to` degrees, clockwise from its right; closed
    /// when it goes all the way round.
    Ellipse {
        centre: Vec2,
        radii: Vec2,
        from: f32,
        to: f32,
    },
    /// The active player's eight rays, each a triangle from the rim of an oval to its tip:
    /// `RAYS` in `Player.tsx`.
    Rays,
}

/// How a layer's coverage fades, as the web's gradient for its color does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ramp {
    /// No fading.
    None,
    /// Down the bezel's gradient (`orb-rim-*` in `LifeOrb`), which runs from the top of the
    /// core's box, 0, to its foot, 1, on the screen for both seats: none above `[0]`, rising
    /// to whole at `[1]`, whole until `[2]` and gone by `[3]`. Equal ends are a step.
    Down([f32; 4]),
    /// Out from the orb's middle (`orb-ray`): whole within `whole` units and gone by `gone`.
    Out { whole: f32, gone: f32 },
}

/// One layer of the art.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layer {
    pub shape: Shape,
    pub paint: CoveragePaint,
    pub ramp: Ramp,
    /// The standard deviation of the blur over it, in units, as `feGaussianBlur`'s.
    pub blur: f32,
    /// How far it is moved down the screen, in units, for both seats.
    pub drop: f32,
    pub color: [u8; 4],
    /// Whether only the active player's orb shows it.
    pub active: bool,
    /// The box its cell covers, in units on the screen, the viewer's orb's and then the
    /// opponent's: its left, top, width and height. It holds all its ink, blur included, and
    /// for a layer of the bezel no more than the band of the gradient it shows.
    pub views: [[f32; 4]; 2],
}

const FILL: CoveragePaint = CoveragePaint {
    fill: true,
    stroke: None,
};

/// The gold ring under the core: `RING`, `rim(5, 37, 143)`.
const RING: Shape = Shape::Rim {
    offset: 5.0,
    from: 37.0,
    to: 143.0,
};

/// The bezel: the core's outline stroked `BEZEL` wide.
const BEZEL_PAINT: CoveragePaint = CoveragePaint {
    fill: false,
    stroke: Some((BEZEL, false)),
};

/// The flare's glow and the bloom inside the core: ellipses all the way round.
const fn oval(cx: f32, cy: f32, rx: f32, ry: f32) -> Shape {
    Shape::Ellipse {
        centre: Vec2::new(cx, cy),
        radii: Vec2::new(rx, ry),
        from: 0.0,
        to: 360.0,
    }
}

/// A layer with no ramp, blur or drop, which both orbs show.
const fn plain(shape: Shape, paint: CoveragePaint, color: [u8; 4], views: [[f32; 4]; 2]) -> Layer {
    Layer {
        shape,
        paint,
        ramp: Ramp::None,
        blur: 0.0,
        drop: 0.0,
        color,
        active: false,
        views,
    }
}

/// `layer` blurred by `blur` units.
const fn blurred(mut layer: Layer, blur: f32) -> Layer {
    layer.blur = blur;
    layer
}

/// `layer`, shown only on the active player's orb.
const fn lit(mut layer: Layer) -> Layer {
    layer.active = true;
    layer
}

/// `layer` faded by `ramp`.
const fn ramped(mut layer: Layer, ramp: Ramp) -> Layer {
    layer.ramp = ramp;
    layer
}

/// The box `view` on the viewer's orb, and mirrored top to bottom on the opponent's.
const fn mirrored(view: [f32; 4]) -> [[f32; 4]; 2] {
    [view, [view[0], -(view[1] + view[3]), view[2], view[3]]]
}

/// A layer of the bezel, in `color`, over the band `ramp` of its gradient that it shows.
const fn bezel(color: [u8; 4], ramp: [f32; 4], views: [[f32; 4]; 2]) -> Layer {
    ramped(
        plain(Shape::Core, BEZEL_PAINT, color, views),
        Ramp::Down(ramp),
    )
}

/// The ray gradient's band from `whole` to `gone` units, in `color`.
const fn rays(color: [u8; 4], whole: f32, gone: f32) -> Layer {
    lit(ramped(
        plain(
            Shape::Rays,
            FILL,
            color,
            mirrored([-69.0, -61.0, 138.0, 43.0]),
        ),
        Ramp::Out { whole, gone },
    ))
}

/// The radius of the ray gradient, `orb-ray`'s `r`: its stops are fractions of it.
const RAY_RADIUS: f32 = 120.0;

/// The layers, painted in order, as `LifeOrb` paints its elements. The colors are its
/// stylesheet's and gradient stops'.
pub const LAYERS: [Layer; 16] = [
    // `.life-orb-flare`'s ellipse, #ff7a14 at 75% opacity, blurred (`orb-blur`).
    lit(blurred(
        plain(
            oval(0.0, -6.0, 56.0, 46.0),
            FILL,
            [0xff, 0x7a, 0x14, 191],
            mirrored([-65.0, -61.0, 130.0, 110.0]),
        ),
        3.0,
    )),
    // `.life-orb-flare`'s rays, #ff6a00, blurred.
    lit(blurred(
        plain(
            Shape::Rays,
            FILL,
            [0xff, 0x6a, 0x00, 255],
            mirrored([-77.0, -69.0, 154.0, 59.0]),
        ),
        3.0,
    )),
    // The rays in `orb-ray`, whose stops are #fff3c0 at 0.3, #ffb030 at 0.42, #f05a10 at 0.6
    // and #c83008 clear at 1: three layers, outermost first. The last fades in #f05a10 where
    // the web's turns to #c83008 as it fades, a little past most of the rays' tips.
    rays([0xf0, 0x5a, 0x10, 255], 0.6 * RAY_RADIUS, RAY_RADIUS),
    rays([0xff, 0xb0, 0x30, 255], 0.42 * RAY_RADIUS, 0.6 * RAY_RADIUS),
    rays([0xff, 0xf3, 0xc0, 255], 0.3 * RAY_RADIUS, 0.42 * RAY_RADIUS),
    // `.life-orb-halo`: the arc `M-42 -17 A47 37 0 0 1 42 -17`, #ff8a14 16 wide, blurred.
    // Its ellipse's centre is where that arc's end points put it.
    lit(blurred(
        plain(
            Shape::Ellipse {
                centre: Vec2::new(0.0, -0.393_28),
                radii: Vec2::new(47.0, 37.0),
                from: -153.331_34,
                to: -26.668_66,
            },
            CoveragePaint {
                fill: false,
                stroke: Some((16.0, false)),
            },
            [0xff, 0x8a, 0x14, 255],
            mirrored([-59.0, -55.0, 118.0, 55.0]),
        ),
        3.0,
    )),
    // The core's drop shadow (`orb-shadow-*`): black at 60%, 2 down the screen, blurred 1.5,
    // under the core and its bezel.
    Layer {
        shape: Shape::Core,
        paint: CoveragePaint {
            fill: true,
            stroke: Some((BEZEL, false)),
        },
        ramp: Ramp::None,
        blur: 1.5,
        drop: 2.0,
        color: [0, 0, 0, 153],
        active: false,
        views: [[-56.0, -45.0, 112.0, 88.0], [-56.0, -39.0, 112.0, 88.0]],
    },
    // `.life-orb-core`'s fill.
    plain(
        Shape::Core,
        FILL,
        kit_base::tokens::color::ORB_CORE,
        mirrored([-50.0, -41.0, 100.0, 76.0]),
    ),
    // The bezel's gradient: #dcd8d2 at 0, #8e8984 at 0.2, #4a4643 at 0.6 and #3a3633 at 1,
    // from the foot's color up.
    bezel(
        [0x3a, 0x36, 0x33, 255],
        [0.6, 0.6, f32::INFINITY, f32::INFINITY],
        [[-53.0, 4.0, 106.0, 33.0], [-53.0, 10.0, 106.0, 33.0]],
    ),
    bezel(
        [0x4a, 0x46, 0x43, 255],
        [0.2, 0.2, 0.6, 1.0],
        [[-53.0, -26.0, 106.0, 60.0], [-53.0, -20.0, 106.0, 60.0]],
    ),
    bezel(
        [0x8e, 0x89, 0x84, 255],
        [0.0, 0.0, 0.2, 0.6],
        [[-53.0, -40.0, 106.0, 45.0], [-53.0, -34.0, 106.0, 45.0]],
    ),
    bezel(
        [0xdc, 0xd8, 0xd2, 255],
        [f32::NEG_INFINITY, f32::NEG_INFINITY, 0.0, 0.2],
        [[-53.0, -43.0, 106.0, 18.0], [-53.0, -37.0, 106.0, 18.0]],
    ),
    // `.life-orb-ring-glow`: the ring, #ff7a10 4 wide at 40% opacity, blurred.
    blurred(
        plain(
            RING,
            CoveragePaint {
                fill: false,
                stroke: Some((4.0, true)),
            },
            [0xff, 0x7a, 0x10, 102],
            mirrored([-55.0, 13.0, 110.0, 37.0]),
        ),
        3.0,
    ),
    // `.life-orb-ring`: #ffb444, 3 wide, round ends.
    plain(
        RING,
        CoveragePaint {
            fill: false,
            stroke: Some((3.0, true)),
        },
        [0xff, 0xb4, 0x44, 255],
        mirrored([-47.0, 21.0, 94.0, 21.0]),
    ),
    // `.life-orb-bloom`: an ellipse at the top of the core, #ffc764, blurred.
    lit(blurred(
        plain(
            oval(0.0, CORE_CY - CORE_RY, 30.0, 8.0),
            FILL,
            [0xff, 0xc7, 0x64, 255],
            mirrored([-39.0, -57.0, 78.0, 34.0]),
        ),
        3.0,
    )),
    // `.life-orb-crescent`: `rim(-4.5, 222, 318)`, #ffe28a 4 wide, round ends.
    lit(plain(
        Shape::Rim {
            offset: -4.5,
            from: 222.0,
            to: 318.0,
        },
        CoveragePaint {
            fill: false,
            stroke: Some((4.0, true)),
        },
        [0xff, 0xe2, 0x8a, 255],
        mirrored([-38.0, -39.0, 76.0, 17.0]),
    )),
];

/// How many icons the art takes: one for each layer of each seat's orb.
pub const ICONS: u16 = 2 * LAYERS.len() as u16;

/// The two orientations of the drawing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    Upright,
    Mirrored,
}

/// Orientations in atlas order.
pub const ORIENTATIONS: [Orientation; 2] = [Orientation::Upright, Orientation::Mirrored];

/// The index of `side` in the pairs above, the viewer's first.
#[must_use]
pub const fn seat(side: Orientation) -> usize {
    match side {
        Orientation::Upright => 0,
        Orientation::Mirrored => 1,
    }
}

/// The first icon reserved for a life-orb layer, after the widget icons.
pub const FIRST_ICON: u16 = icon_artwork::COUNT;

/// The icon of layer `at` of `side`'s orb.
#[must_use]
pub const fn icon(side: Orientation, at: usize) -> IconId {
    #[expect(clippy::cast_possible_truncation, reason = "sixteen layers")]
    IconId(FIRST_ICON + (seat(side) * LAYERS.len() + at) as u16)
}

const fn icon_layers(side: Orientation) -> [IconLayer; LAYERS.len()] {
    let mut layers = [IconLayer {
        icon: IconId(0),
        view: [0.0; 4],
        color: [0; 4],
        highlight: false,
    }; LAYERS.len()];
    let mut at = 0;
    while at < LAYERS.len() {
        let layer = &LAYERS[at];
        layers[at] = IconLayer {
            icon: icon(side, at),
            view: layer.views[seat(side)],
            color: layer.color,
            highlight: layer.active,
        };
        at += 1;
    }
    layers
}

/// The art filling an orb's box, lit if its seat is active.
#[must_use]
pub fn artwork(side: Side, active: bool, scale: u8) -> LayeredIcon {
    const VIEWER: [IconLayer; LAYERS.len()] = icon_layers(Orientation::Upright);
    const OPPONENT: [IconLayer; LAYERS.len()] = icon_layers(Orientation::Mirrored);
    LayeredIcon {
        layers: match orientation(side) {
            Orientation::Upright => &VIEWER,
            Orientation::Mirrored => &OPPONENT,
        },
        origin: [ART[0], ART[1]],
        scale: SCALES[usize::from(scale)],
        highlighted: active,
    }
}

const fn orientation(side: Side) -> Orientation {
    match side {
        Side::You => Orientation::Upright,
        Side::Opponent => Orientation::Mirrored,
    }
}

/// Every choice of art scale, orientation and layer, in the order sent to the atlas.
fn atlas_layers() -> impl Iterator<Item = (Orientation, &'static Layer, LayerCell)> {
    SCALES.into_iter().flat_map(|units| {
        ORIENTATIONS.into_iter().flat_map(move |side| {
            LAYERS.iter().enumerate().map(move |(at, layer)| {
                let view = layer.views[seat(side)];
                let cell = LayerCell {
                    icon: icon(side, at),
                    view,
                    size: cell_width(view[2], units),
                };
                (side, layer, cell)
            })
        })
    })
}

/// The atlas dimensions for every orb layer, for a driver that does not rasterize them.
pub fn atlas_cells() -> impl Iterator<Item = LayerCell> {
    atlas_layers().map(|(_, _, cell)| cell)
}

/// Describes the orb layers for the asset worker, in their existing atlas order.
#[must_use]
pub fn atlas_artwork() -> Vec<CoverageLayer> {
    atlas_layers()
        .map(|(side, layer, cell)| CoverageLayer {
            cell,
            outlines: outlines(layer.shape, side),
            paint: layer.paint,
            ramp: match layer.ramp {
                Ramp::None => CoverageRamp::None,
                Ramp::Down(stops) => CoverageRamp::Vertical {
                    top: core_top(side),
                    height: 2.0 * CORE_RY,
                    stops,
                },
                Ramp::Out { whole, gone } => CoverageRamp::Radial { whole, gone },
            },
            blur: layer.blur,
            drop: layer.drop,
        })
        .collect()
}

/// The height in units of the top of the core's box on `side`'s orb, from which the bezel's
/// gradient runs down: the opponent's core is mirrored about the orb's middle.
#[must_use]
pub fn core_top(side: Orientation) -> f32 {
    match side {
        Orientation::Upright => CORE_CY - CORE_RY,
        Orientation::Mirrored => -CORE_CY - CORE_RY,
    }
}

/// The point of the core's outline at `t` radians clockwise from its right: `corePoint`.
#[must_use]
pub fn core_point(t: f32) -> Vec2 {
    let (s, c) = t.sin_cos();
    let e = 2.0 / SQUARENESS;
    Vec2::new(
        CORE_RX * c.signum() * c.abs().powf(e),
        CORE_CY + CORE_RY * s.signum() * s.abs().powf(e),
    )
}

/// The outlines of `shape` on `side`'s orb, in units on the screen, each with whether it is
/// closed. The opponent's are the viewer's mirrored top to bottom.
#[must_use]
pub fn outlines(shape: Shape, side: Orientation) -> Vec<(Vec<Vec2>, bool)> {
    let mut lines = match shape {
        Shape::Core => vec![(
            (0..72)
                .map(|i| core_point(i as f32 / 72.0 * std::f32::consts::TAU))
                .collect(),
            true,
        )],
        Shape::Rim { offset, from, to } => vec![(rim(offset, from, to), false)],
        Shape::Ellipse {
            centre,
            radii,
            from,
            to,
        } => {
            let whole = (to - from).abs() >= 360.0;
            let steps = 64;
            let points = (0..=steps)
                .map(|i| {
                    let t = (from + (to - from) * i as f32 / steps as f32).to_radians();
                    centre + radii * Vec2::new(t.cos(), t.sin())
                })
                .collect();
            vec![(points, whole)]
        }
        Shape::Rays => ray_tips()
            .into_iter()
            .map(|tip| {
                let a = tip.y.atan2(tip.x / 1.2);
                let base = |da: f32| Vec2::new((a + da).cos() * 36.0 * 1.2, (a + da).sin() * 36.0);
                (vec![base(-0.06), tip, base(0.06)], true)
            })
            .collect(),
    };
    if side == Orientation::Mirrored {
        for (points, _) in &mut lines {
            for point in points {
                point.y = -point.y;
            }
        }
    }
    lines
}

/// The part of the core's outline from `from` to `to` degrees, moved `offset` units out along
/// its normal: an arc that follows the cushion's shape all along it, as an ellipse's does not.
fn rim(offset: f32, from: f32, to: f32) -> Vec<Vec2> {
    let steps = 32;
    (0..=steps)
        .map(|i| {
            let t = (from + (to - from) * i as f32 / steps as f32).to_radians();
            let point = core_point(t);
            let tangent = core_point(t + 0.001) - core_point(t - 0.001);
            // Clockwise on screen, the outward normal is the tangent turned a quarter back.
            let normal = Vec2::new(tangent.y, -tangent.x).normalize();
            point + normal * offset
        })
        .collect()
}

/// The tips of the eight rays: the left half's, each mirrored onto the right.
fn ray_tips() -> Vec<Vec2> {
    [
        (-16.0, -60.0),
        (-36.0, -58.0),
        (-54.0, -52.0),
        (-68.0, -42.0),
    ]
    .into_iter()
    .flat_map(|(x, y)| [Vec2::new(x, y), Vec2::new(-x, y)])
    .collect()
}

#[cfg(test)]
mod tests {

    use super::{LAYERS, Orientation, Ramp, core_top, outlines};

    /// Each layer's box holds all its ink: the outlines, half its stroke, three standard
    /// deviations of its blur and its drop, cut to the band of a bezel's gradient it shows.
    /// A box that cut the ink off would show a hard edge.
    #[test]
    fn every_layer_box_holds_its_ink() {
        for (at, layer) in LAYERS.iter().enumerate() {
            for side in [Orientation::Upright, Orientation::Mirrored] {
                let margin =
                    layer.paint.stroke.map_or(0.0, |(width, _)| width / 2.0) + 3.0 * layer.blur;
                let points = outlines(layer.shape, side).into_iter().flat_map(|(p, _)| p);
                let (mut low, mut high) = (glam::Vec2::MAX, glam::Vec2::MIN);
                for point in points {
                    low = low.min(point);
                    high = high.max(point);
                }
                let (mut top, mut bottom) = (low.y - margin, high.y + margin);
                top += layer.drop;
                bottom += layer.drop;
                if let Ramp::Down(band) = layer.ramp {
                    top = top.max(core_top(side) + band[0] * 74.0);
                    bottom = bottom.min(core_top(side) + band[3] * 74.0);
                }
                let [x, y, w, h] = layer.views[super::seat(side)];
                let slack = 1.0;
                assert!(
                    x <= low.x - margin + slack
                        && x + w >= high.x + margin - slack
                        && y <= top + slack
                        && y + h >= bottom - slack,
                    "layer {at} on {side:?}'s orb: box {:?} misses ink from {:?} to {:?}",
                    [x, y, w, h],
                    (low.x - margin, top),
                    (high.x + margin, bottom),
                );
            }
        }
    }
}
