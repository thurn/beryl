//! The desktop layout's battlefield rows, fitted to a window so that each card lands where the
//! web interface draws it (docs/beryl-3d.md, "Matching the web interface's look").
//!
//! The web interface lays the table out on a stage 900 pixels high, scaled to fit the window
//! (`ui/src/game/Stage.tsx`), and draws each card as an upright rectangle shrunk by how far up
//! the stage it lies and leaned toward a vanishing point over the stage's middle
//! (`ui/src/game/perspective.ts`). Its rows are stage positions: `ROWS` in
//! `ui/src/game/tableRows.ts` and `sideRows` in `ui/src/game/battlefield.ts`, copied into
//! `layout.desktop.web` in `space.json`.
//!
//! Beryl 3D keeps its own camera, fitted to Arena (`layout.desktop.camera`), and lays its cards
//! flat on a table. For each row it finds the line of the table under the web's cards' centres,
//! the row's ends, and the scale at which a card is as wide on screen as the web's, so that each
//! card's centre projects onto the centre of the web's card. A row is laid out in millimetres of
//! a card at its printed size and then scaled by that scale about its left end (`place::row`).
//! A card lying flat under the camera's pitch is shorter on screen against its width than the
//! web's upright one, so each row's cards lean back toward the camera until they are as tall.

use glam::{Vec2, Vec3};

use super::{CARD_HEIGHT, CARD_WIDTH, Camera, RowArea, Window};
use crate::data::space::layout::desktop::web as data;

/// The web interface's `CARD_ASPECT`, a card's width over its height.
const ASPECT: f32 = CARD_WIDTH / CARD_HEIGHT;

/// One battlefield row of one side fitted to the web's: its area on the table, the scale its
/// cards are drawn at, and the gaps between them in millimetres before that scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FittedRow {
    /// The row's band, in millimetres of the table. Its `y` is the line the centres of its
    /// untapped cards lie on, signed for its side, and its `left` and `right` are where the
    /// edges of its first and last cards may reach along that line.
    pub area: RowArea,
    /// How large its cards are against a card's printed size.
    pub scale: f32,
    /// How far apart its cards stand, before `scale`.
    pub gap: f32,
    /// How far its first tapped card stands from the untapped card before it, before `scale`:
    /// the web's `TAPPED_GAP` in the lands' row, and `gap` in the others.
    pub tapped_gap: f32,
    /// How far its cards lean back from lying flat, toward the camera, in radians: as far as
    /// makes an untapped card as tall on screen as the web's upright one, which a card lying flat
    /// under the camera's pitch is not. Every card of the row lies in one plane, which rises
    /// toward the far side of the table at this angle from the near edge of an untapped card
    /// (`FittedRow::height`), so that a card placed later lies over one placed before it,
    /// wherever they overlap.
    pub tilt: f32,
}

impl FittedRow {
    /// How high above the table the row's plane lies over `y`, at its cards' scale.
    #[must_use]
    pub fn height(&self, y: f32) -> f32 {
        let half = CARD_HEIGHT / 2.0 * self.scale;
        half * self.tilt.sin() + (y - self.area.y) * self.tilt.tan()
    }
}

/// Each side's three rows, the viewer's first, each in the order of `place::ROWS`.
pub type FittedRows = [[FittedRow; 3]; 2];

/// The web interface's stage as it is fitted to a window (`fit` in `ui/src/game/Stage.tsx`).
struct Stage {
    /// How wide the stage is, in stage pixels: as wide as the window's shape asks, within
    /// `STAGE_WIDTHS`.
    width: f32,
    /// Logical points per stage pixel.
    scale: f32,
    /// How far the stage's top left corner lies from the window's, in stage pixels.
    offset: Vec2,
}

impl Stage {
    fn of(window: Window) -> Self {
        let size = window.size;
        let width = (data::stage::HEIGHT * size.width / size.height)
            .clamp(data::stage::LEAST, data::stage::MOST);
        let scale = (size.width / width).min(size.height / data::stage::HEIGHT);
        Self {
            width,
            scale,
            offset: Vec2::new(
                (size.width / scale - width) / 2.0,
                (size.height / scale - data::stage::HEIGHT) / 2.0,
            ),
        }
    }

    /// A point of the stage on the window, in logical points.
    fn on_window(&self, point: Vec2) -> Vec2 {
        (point + self.offset) * self.scale
    }

    /// The web's `fromRight`: what stands at the right keeps its distance from the right edge.
    fn right_held(&self, x: f32) -> f32 {
        x + self.width - data::stage::BASE
    }

    /// The web's `fromMiddle`: what stands at the middle keeps its distance from the middle.
    fn middle_held(&self, x: f32) -> f32 {
        x + (self.width - data::stage::BASE) / 2.0
    }

    /// The tangent of the web's `lean` of a thing whose top centre is at (`x`, `y`): how far its
    /// sides run sideways for each pixel down.
    fn lean(&self, x: f32, y: f32) -> f32 {
        (x - self.width / 2.0) / (y - data::perspective::VANISHING_Y)
    }
}

/// The web's `depthScale`: how much a thing lying at stage height `y` is shrunk by distance.
fn depth_scale(y: f32) -> f32 {
    use data::perspective::{DEPTH_PER_PX, DISTANCE, NEAR_Y};
    DISTANCE / (DISTANCE + (NEAR_Y - y).max(0.0) * DEPTH_PER_PX)
}

/// One of the web's rows of permanents (`sideRows`), in stage pixels at full zoom.
struct RowSpec {
    /// Where its cards' tops lie.
    top: f32,
    /// Where its first card's left edge may stand, and its last card's right edge.
    start: f32,
    end: f32,
    /// Where it is centred while it fits there; none for a row laid from `start`.
    centre: Option<f32>,
    /// A card's frame width, before depth scaling.
    frame: f32,
    /// The gaps between cards, and before the first tapped card, before depth scaling.
    gap: f32,
    tapped_gap: f32,
}

/// The rows of `side` (the viewer's first) fitted to `window` through `camera`.
fn side_rows(stage: &Stage, camera: &Camera, opponent: bool) -> [FittedRow; 3] {
    macro_rules! specs {
        ($side:ident) => {{
            use data::$side as rows;
            let creatures = RowSpec {
                top: rows::PERMANENTS,
                start: data::room::START,
                end: stage.right_held(data::room::END),
                centre: Some(stage.width / 2.0),
                frame: data::CREATURE_WIDTH,
                gap: data::PERMANENT_GAP * data::CREATURE_WIDTH,
                tapped_gap: data::PERMANENT_GAP * data::CREATURE_WIDTH,
            };
            let lands = RowSpec {
                top: rows::BACK,
                start: rows::LANDS,
                end: stage.middle_held(rows::LANDS_END),
                centre: None,
                frame: data::LAND_WIDTH,
                gap: data::LAND_GAP,
                tapped_gap: data::TAPPED_GAP,
            };
            let others = RowSpec {
                top: rows::BACK,
                start: stage.middle_held(rows::OTHERS_X),
                end: stage.right_held(rows::OTHERS_END),
                centre: None,
                frame: rows::OTHERS_WIDTH,
                gap: data::PERMANENT_GAP * rows::OTHERS_WIDTH,
                tapped_gap: data::PERMANENT_GAP * rows::OTHERS_WIDTH,
            };
            [creatures, lands, others]
        }};
    }
    let specs = if opponent {
        specs!(opponent)
    } else {
        specs!(you)
    };
    specs.map(|spec| fit_row(stage, camera, &spec))
}

/// `spec` fitted through `camera`. Along a line of the screen, a card's place across the table
/// is linear in its place across the screen, as a point's place across the web's stage is linear
/// in the x of the card it is the centre of: the web's lean moves a card's centre sideways by an
/// amount linear in its x. So the row's ends, its scale and its gaps follow from where two of its
/// points land. Its line and its cards' tilt are found by `Stand::fit`.
fn fit_row(stage: &Stage, camera: &Camera, spec: &RowSpec) -> FittedRow {
    let width = spec.frame * depth_scale(spec.top);
    let height = width / ASPECT;
    let line = stage.on_window(Vec2::new(0.0, spec.top + height / 2.0)).y;
    // The centre of the web's card whose top left corner is at `x`, leaned about its top.
    let centre_of = |x: f32| x + width / 2.0 + stage.lean(x + width / 2.0, spec.top) * height / 2.0;
    let across = |x: f32, lift: f32| {
        let point = stage.on_window(Vec2::new(x, 0.0));
        camera.on_plane(Vec2::new(point.x, line), lift).x
    };
    // Millimetres of the table for each stage pixel between two cards' left edges, at `lift`.
    let per_pixel = |lift: f32| {
        (across(centre_of(spec.end - width), lift) - across(centre_of(spec.start), lift))
            / (spec.end - width - spec.start)
    };
    let mut stand = Stand {
        y: camera.on_plane(Vec2::new(0.0, line), 0.0).y,
        tilt: 0.0,
        scale: width * per_pixel(0.0) / CARD_WIDTH,
    };
    // The scale and the stand depend on each other only a little: two rounds settle both.
    for _ in 0..2 {
        stand = stand.fit(camera, line, height * stage.scale);
        stand.scale = width * per_pixel(stand.lift(CARD_HEIGHT)) / CARD_WIDTH;
    }
    let lift = stand.lift(CARD_HEIGHT);
    let scale = stand.scale;
    // A gap of the web's, against its card's frame, is the same share of a card here.
    let gap = |pixels: f32| CARD_WIDTH * pixels / spec.frame;
    FittedRow {
        area: RowArea {
            y: stand.y,
            left: across(centre_of(spec.start), lift) - CARD_WIDTH * scale / 2.0,
            right: across(centre_of(spec.end - width), lift) + CARD_WIDTH * scale / 2.0,
            centre: spec.centre.map(|x| across(x, lift)),
        },
        scale,
        gap: gap(spec.gap),
        tapped_gap: gap(spec.tapped_gap),
        tilt: stand.tilt,
    }
}

/// How the cards of a row stand: the line of the table their centres lie over, how far each
/// leans back from lying flat toward the camera, about the table's x axis, and their scale.
#[derive(Clone, Copy, Debug)]
struct Stand {
    y: f32,
    /// In radians.
    tilt: f32,
    scale: f32,
}

/// The most a row's cards lean toward the camera, in radians: well short of standing upright.
const MOST_TILT: f32 = 1.2;

impl Stand {
    /// How far above the table the centre of a card `length` millimetres long from its near edge
    /// to its far one stands, before the scale, with its near edge on the table.
    fn lift(self, length: f32) -> f32 {
        length / 2.0 * self.scale * self.tilt.sin()
    }

    /// The top and bottom on screen, in logical points, of a card `length` millimetres long
    /// whose centre lies `up` millimetres over `y`, leaning back.
    fn extent(self, camera: &Camera, (y, up): (f32, f32), length: f32) -> (f32, f32) {
        let half = length / 2.0 * self.scale;
        let (along, rise) = (half * self.tilt.cos(), half * self.tilt.sin());
        let top = camera.to_screen(Vec3::new(0.0, y + along, up + rise)).y;
        let bottom = camera.to_screen(Vec3::new(0.0, y - along, up - rise)).y;
        (top, bottom)
    }

    /// `extent` of an untapped card whose centre lies over `y`, with its near edge on the table.
    fn untapped(self, camera: &Camera, y: f32) -> (f32, f32) {
        self.extent(camera, (y, self.lift(CARD_HEIGHT)), CARD_HEIGHT)
    }

    /// The stand whose untapped card is `height` points tall on screen with its middle on the
    /// screen's line `line`, as the web's upright card is: the tilt found by bisection, the line
    /// of the table by the secant method, each in turn until both settle.
    fn fit(self, camera: &Camera, line: f32, height: f32) -> Stand {
        let mut stand = self;
        for _ in 0..4 {
            let (mut low, mut high) = (0.0, MOST_TILT);
            for _ in 0..24 {
                let tilt = f32::midpoint(low, high);
                let (top, bottom) = Stand { tilt, ..stand }.untapped(camera, stand.y);
                if bottom - top < height {
                    low = tilt;
                } else {
                    high = tilt;
                }
            }
            stand.tilt = low;
            stand.y = Stand::solve(stand.y, |y| {
                let (top, bottom) = stand.untapped(camera, y);
                f32::midpoint(top, bottom) - line
            });
        }
        stand
    }

    /// The `y` near `start` where `miss` is nought, by the secant method.
    fn solve(start: f32, miss: impl Fn(f32) -> f32) -> f32 {
        let (mut a, mut b) = (start, start + 1.0);
        let (mut miss_a, mut miss_b) = (miss(a), miss(b));
        for _ in 0..8 {
            if (miss_b - miss_a).abs() < f32::EPSILON {
                break;
            }
            let next = b - miss_b * (b - a) / (miss_b - miss_a);
            (a, miss_a) = (b, miss_b);
            (b, miss_b) = (next, miss(next));
        }
        b
    }
}

/// Every row of the desktop layout fitted to `window` through `camera`.
pub(super) fn rows(window: Window, camera: &Camera) -> FittedRows {
    let stage = Stage::of(window);
    [false, true].map(|opponent| side_rows(&stage, camera, opponent))
}
