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

use super::pile::lift;
use super::{CARD_HEIGHT, CARD_WIDTH, Camera, LAYER, RowArea, Window};
use crate::data::space::layout::desktop::web as data;
use crate::pose::Pose;

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
    /// How far along the table an attacking card's centre moves from the row's line, signed,
    /// untapped then tapped. The web moves an attacker `ADVANCE` stage pixels toward the other
    /// side (`permanentRow` in `ui/src/game/battlefield.ts`), and lines a tapped card up with
    /// the row by its top, where Beryl 3D keeps it centred on the row's line, so a tapped
    /// attacker moves as far again as makes its centre the web's. Cards that do not attack keep
    /// to the line, for the hit test's sake (`place::row`): an attacker stands out of its row.
    pub advance: [f32; 2],
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
    // Toward the other side: up the stage on the viewer's side, down it on the opponent's.
    let forward = if opponent {
        data::ADVANCE
    } else {
        -data::ADVANCE
    };
    specs.map(|spec| fit_row(stage, camera, &spec, forward))
}

/// `spec` fitted through `camera`. Along a line of the screen, a card's place across the table
/// is linear in its place across the screen, as a point's place across the web's stage is linear
/// in the x of the card it is the centre of: the web's lean moves a card's centre sideways by an
/// amount linear in its x. So the row's ends, its scale and its gaps follow from where two of its
/// points land. Its line and its cards' tilt are found by `Stand::fit`.
fn fit_row(stage: &Stage, camera: &Camera, spec: &RowSpec, forward: f32) -> FittedRow {
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
    let mut row = FittedRow {
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
        advance: [0.0; 2],
    };
    // A tapped card's centre lies half the difference of its height and width higher on the
    // web's stage than an untapped one's, both tops on the row's top.
    let tapped = forward - (height - width) / 2.0;
    row.advance = [forward, tapped].map(|pixels| {
        let goal = line + pixels * stage.scale;
        let moved = Stand::solve(row.area.y, |y| {
            camera.to_screen(Vec3::new(0.0, y, row.height(y))).y - goal
        });
        moved - row.area.y
    });
    row
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
    /// of the table by the secant method, each in turn until both settle. A card grows taller on
    /// screen as it leans back only until it faces the camera, and shorter past that, so the
    /// bisection looks no further than that tilt. Near the bottom of the window, where the camera
    /// looks down more steeply, it faces the camera after a small lean, and a card that cannot
    /// grow as tall as `height` stands at that tilt, as tall as it can be.
    fn fit(self, camera: &Camera, line: f32, height: f32) -> Stand {
        let mut stand = self;
        for _ in 0..4 {
            let tall = |tilt: f32| {
                let (top, bottom) = Stand { tilt, ..stand }.untapped(camera, stand.y);
                bottom - top
            };
            // The tallest tilt, by ternary search.
            let (mut low, mut high) = (0.0, MOST_TILT);
            for _ in 0..32 {
                let (a, b) = (low + (high - low) / 3.0, high - (high - low) / 3.0);
                if tall(a) < tall(b) {
                    low = a;
                } else {
                    high = b;
                }
            }
            let (mut low, mut high) = (0.0, f32::midpoint(low, high));
            for _ in 0..24 {
                let tilt = f32::midpoint(low, high);
                if tall(tilt) < height {
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

/// The library, graveyard and exile piles of the desktop layout fitted to a window, so that
/// each pile's top card lands where the web interface draws it (`pileLook` in
/// `ui/src/game/pileLook.ts`). The web keeps a pile's top card at its anchor however many cards
/// the pile holds, so a pile here stands wherever puts its top card's centre under `tops`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FittedPiles {
    /// The middle of each pile's top card on the window, in logical points: by side, the
    /// viewer's first, then library, graveyard and exile.
    pub tops: [[Vec2; 3]; 2],
    /// How large each side's pile cards are against a card's printed size, the viewer's first.
    pub scale: [f32; 2],
    /// How far each side's pile cards lean back from lying flat toward the camera, in radians,
    /// as a fitted row's do (`FittedRow::tilt`), so that a top card is as tall on screen as the
    /// web's upright one.
    pub tilt: [f32; 2],
    /// How far each pile's cards are sheared, by side (the viewer's first) then library,
    /// graveyard and exile: the tangent of the web's `lean` of the pile's top card, as
    /// `Pose::skew` takes it.
    pub skew: [[f32; 3]; 2],
}

/// Every pile of the desktop layout fitted to `window` through `camera`. The web draws a pile's
/// top card as it draws a permanent: upright, shrunk by how far up the stage its anchor lies and
/// leaned toward the vanishing point by a skew about its top left corner (`PileButton` in
/// `ui/src/game/Piles.tsx`).
pub(super) fn piles(window: Window, camera: &Camera) -> FittedPiles {
    use data::piles;
    let stage = Stage::of(window);
    let mut fitted = FittedPiles {
        tops: [[Vec2::ZERO; 3]; 2],
        scale: [1.0; 2],
        tilt: [0.0; 2],
        skew: [[0.0; 3]; 2],
    };
    for side in 0..2 {
        // The anchor's height on the stage, the card's unscaled width, the anchors' x, and how
        // far the top card lies below its anchor (`PILE_BASE`). The opponent's top cards lie
        // so that the deepest layer of the thickest pile, which spreads downward, ends
        // `clearance` above their back row.
        let (y, frame, xs, base) = if side == 0 {
            let xs = piles::you::X.to_array();
            (piles::you::Y, piles::you::WIDTH, xs, piles::you::BASE)
        } else {
            let xs = piles::opponent::X.to_array().map(|x| stage.right_held(x));
            let height = piles::opponent::WIDTH * depth_scale(piles::opponent::Y) / ASPECT;
            let base = data::opponent::BACK
                - piles::CLEARANCE
                - piles::opponent::SPREAD_DOWN * piles::MOST_THICKNESS
                - piles::opponent::Y
                - height;
            (piles::opponent::Y, piles::opponent::WIDTH, xs, base)
        };
        let width = frame * depth_scale(y);
        let height = width / ASPECT;
        for (zone, x) in xs.into_iter().enumerate() {
            let middle = x + width / 2.0;
            let centre = Vec2::new(
                middle + stage.lean(middle, y) * height / 2.0,
                y + base + height / 2.0,
            );
            fitted.tops[side][zone] = stage.on_window(centre);
        }
        let stand = fit_card(
            camera,
            fitted.tops[side][0].y,
            (width * stage.scale, height * stage.scale),
        );
        fitted.scale[side] = stand.scale;
        fitted.tilt[side] = stand.tilt;
        // The camera already leans a card toward the vanishing point the further it lies from
        // the middle of the screen, so a card is sheared only by what the web's lean has over
        // that.
        for (zone, x) in xs.into_iter().enumerate() {
            let middle = x + width / 2.0;
            let top = fitted.tops[side][zone];
            let z = LAYER * 0.5 + lift(stand.scale, stand.tilt);
            let pose = Pose {
                position: camera.on_plane(top, z).truncate().extend(z),
                rotation: glam::Quat::from_rotation_x(stand.tilt),
                scale: stand.scale,
                ..Pose::default()
            };
            let [bottom_left, bottom_right, top_right, top_left] =
                camera.card_corners(pose.transform());
            let (head, foot) = (
                top_left.midpoint(top_right),
                bottom_left.midpoint(bottom_right),
            );
            let natural = (foot.x - head.x) / (foot.y - head.y);
            let skew = stage.lean(middle, y) - natural;
            fitted.skew[side][zone] = skew;
            // The camera's perspective moves the middle of a tilted card's outline a little off
            // the screen point its middle stands under, and the web's boxes are measured by
            // their outline, so the pile moves by what the outline is off.
            let sheared = Pose { skew, ..pose };
            let corners = camera.card_corners(sheared.transform());
            let low = corners
                .iter()
                .fold(Vec2::MAX, |low, corner| low.min(*corner));
            let high = corners
                .iter()
                .fold(Vec2::MIN, |high, corner| high.max(*corner));
            fitted.tops[side][zone] += top - low.midpoint(high);
        }
    }
    fitted
}

/// The stand of a card whose middle lies on the screen's line `line` and which is `size`
/// points wide and tall on screen there, as the web's upright card is. Under a camera that is
/// neither turned nor rolled, how wide and tall a card is on screen does not depend on where
/// across the table it lies. The scale and the stand depend on each other only a little: a few
/// rounds settle both.
fn fit_card(camera: &Camera, line: f32, (width, height): (f32, f32)) -> Stand {
    let mut stand = Stand {
        y: camera.on_plane(Vec2::new(0.0, line), 0.0).y,
        tilt: 0.0,
        scale: 1.0,
    };
    for _ in 0..4 {
        stand = stand.fit(camera, line, height);
        let up = stand.lift(CARD_HEIGHT);
        let half = CARD_WIDTH / 2.0 * stand.scale;
        let left = camera.to_screen(Vec3::new(-half, stand.y, up)).x;
        let right = camera.to_screen(Vec3::new(half, stand.y, up)).x;
        stand.scale *= width / (right - left);
    }
    stand.fit(camera, line, height)
}
